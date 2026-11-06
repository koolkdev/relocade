mod common;

use relocade_dos::{LoadError, LoadOptions, ProcessStatus};
use relocade_x86::{ExecutionExit, HostExit};

// DS points to the PSP at both COM and MZ entry.
const PRINT_TAIL: &[u8] = &[
    0x31, 0xc9, // XOR CX,CX
    0x8a, 0x0e, 0x80, 0, // MOV CL,[0080]: command-tail length
    0xbb, 1, 0, // MOV BX,1: stdout
    0xba, 0x81, 0, // MOV DX,0081: command-tail bytes
    0xb4, 0x40, 0xcd, 0x21, // Write CX bytes from DS:DX.
    0xb8, 0, 0x4c, 0xcd, 0x21, // Exit 0.
];

async fn check_command_tails() {
    let mut dos = common::dos().await;
    let maximum = [b'x'; 126];
    for mz in [false, true] {
        let image = if mz {
            common::mz_image(PRINT_TAIL)
        } else {
            PRINT_TAIL.to_vec()
        };
        for tail in [&b""[..], &b"  \"two words\" \x82"[..], &maximum[..]] {
            let options = LoadOptions {
                command_tail: tail,
                ..Default::default()
            };
            if mz {
                dos.launch_mz(&image, options).unwrap();
            } else {
                dos.launch_com(&image, options).unwrap();
            }
            let mut bytes = [0; 128];
            dos.cpu()
                .context()
                .memory
                .read(0x10080, &mut bytes)
                .unwrap();
            assert_eq!(bytes[0] as usize, tail.len());
            assert_eq!(&bytes[1..1 + tail.len()], tail);
            assert_eq!(bytes[1 + tail.len()], 13);
            assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(
                dos.process().unwrap().status(),
                ProcessStatus::Exited(Ok(0))
            );
            assert_eq!(common::output(dos.stdout()), tail);
        }

        let cpu = dos.cpu().context().cpu.snapshot();
        let mut before = [0; 256];
        dos.cpu()
            .context()
            .memory
            .read(0x10000, &mut before)
            .unwrap();
        let options = LoadOptions {
            command_tail: &[b'x'; 127],
            ..Default::default()
        };
        let result = if mz {
            dos.launch_mz(&image, options)
        } else {
            dos.launch_com(&image, options)
        };
        assert_eq!(result, Err(LoadError::CommandTailTooLong.into()));
        assert_eq!(dos.cpu().context().cpu.snapshot(), cpu);
        let mut after = [0; 256];
        dos.cpu()
            .context()
            .memory
            .read(0x10000, &mut after)
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(0))
        );
    }
}

#[test]
fn com_and_mz_receive_command_tails() {
    pollster::block_on(check_command_tails());
}

#[test]
fn com_and_mz_read_the_inherited_interrupt_vectors_from_their_psp() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let memory = dos.cpu().memory();
        // Copy the six saved PSP words through DS/ES into the unused command
        // tail, which PSP initialization clears again for each launch.
        let program = [
            0xbe, 0x0a, 0, 0xbf, 0x80, 0, 0xb9, 6, 0, 0xf3, 0xa5, // REP MOVSW.
            0xb8, 0, 0x4c, 0xcd, 0x21,
        ];
        let inherited = [0x1357_2468, 0x2345_1357, 0x3456_4321];
        for mz in [false, true] {
            for (index, &pointer) in inherited.iter().enumerate() {
                memory
                    .view()
                    .write_u32((0x22 + index as u32) * 4, pointer)
                    .unwrap();
            }
            if mz {
                dos.launch_mz(&common::mz_image(&program), Default::default())
                    .unwrap();
            } else {
                dos.launch_com(&program, Default::default()).unwrap();
            }
            let psp = u32::from(dos.process().unwrap().psp_segment()) << 4;
            assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(
                dos.process().unwrap().status(),
                ProcessStatus::Exited(Ok(0))
            );
            for (index, &pointer) in inherited.iter().enumerate() {
                assert_eq!(
                    memory
                        .view()
                        .read_u32(psp + 0x80 + index as u32 * 4)
                        .unwrap(),
                    pointer
                );
                assert_eq!(
                    memory.view().read_u32((0x22 + index as u32) * 4).unwrap(),
                    pointer
                );
            }
        }
    });
}

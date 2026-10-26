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
    let mut dos = relocade_dos::Dos::new().await.unwrap();
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
            assert_eq!(
                dos.cpu().run(100).unwrap(),
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(
                dos.process().unwrap().status(),
                ProcessStatus::Exited(Ok(0))
            );
            assert_eq!(common::output(dos.stdout()), tail);
        }

        let cpu_before = dos.cpu().context().cpu.snapshot();
        let memory = dos.memory();
        let mut before = [0; 256];
        memory.view().read(0x10000, &mut before).unwrap();
        let options = LoadOptions {
            command_tail: &[b'x'; 127],
            ..Default::default()
        };
        let result = if mz {
            dos.launch_mz(&image, options)
        } else {
            dos.launch_com(&image, options)
        };
        assert_eq!(common::load_error(result), LoadError::CommandTailTooLong);
        let mut after = [0; 256];
        memory.view().read(0x10000, &mut after).unwrap();
        assert_eq!(after, before);
        assert_eq!(dos.cpu().context().cpu.snapshot(), cpu_before);
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

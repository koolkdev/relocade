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
fn host_process_owns_a_small_dos_allocation_and_terminates_normally() {
    use relocade_dos::LaunchError;
    use relocade_x86::Segment;

    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_host(LoadOptions {
            command_tail: b" /P",
            environment: &[b"COMSPEC=C:\\COMMAND.COM"],
            program_path: b"C:\\COMMAND.COM",
        })
        .unwrap();
        let process = dos.process().unwrap();
        assert!(process.is_host());
        assert_eq!(process.status(), ProcessStatus::Active);
        let segment = process.psp_segment();
        let address = u32::from(segment) * 16;
        let cpu = dos.cpu().cpu_state().snapshot();
        for register in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
            assert_eq!(cpu.segments[register].selector, segment);
        }
        assert_eq!(cpu.eip, 0x100);
        let memory = dos.cpu().memory();
        let mut psp = [0; 256];
        memory.view().read(address, &mut psp).unwrap();
        assert_eq!(&psp[..2], &[0xcd, 0x20]);
        assert_eq!(u16::from_le_bytes([psp[0x16], psp[0x17]]), segment);
        assert_eq!(&psp[0x80..0x85], b"\x03 /P\r");
        let end = u16::from_le_bytes([psp[2], psp[3]]);
        let allocated_bytes = u32::from(end - segment) * 16;
        assert!(cpu.registers.esp <= allocated_bytes);
        assert!(allocated_bytes < 16 * 1024, "leave RAM for DOS allocations");
        let environment = u16::from_le_bytes([psp[0x2c], psp[0x2d]]);
        let expected = b"COMSPEC=C:\\COMMAND.COM\0\0\x01\0C:\\COMMAND.COM\0";
        let mut actual = vec![0; expected.len()];
        memory
            .view()
            .read(u32::from(environment) * 16, &mut actual)
            .unwrap();
        assert_eq!(actual, expected);

        assert_eq!(
            dos.launch_com(&[0xc3], Default::default()),
            Err(LaunchError::ProcessActive)
        );
        assert_eq!(dos.cpu().cpu_state().snapshot(), cpu);
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(process.status(), ProcessStatus::Exited(Ok(0)));
        dos.launch_com(&[0xc3], Default::default()).unwrap();
        assert!(!dos.process().unwrap().is_host());
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
    });
}

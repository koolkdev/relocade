mod common;

use relocade_dos::{LoadError, LoadOptions, ProcessStatus};
use relocade_x86::{ExecutionExit, HostExit};

async fn check_environments() {
    let mut dos = common::dos().await;
    assert!(dos.initial_environment().is_empty()); // AdLib needs no environment entry.
    let options = LoadOptions {
        environment: &[b"NAME=demo", b"PATH=C:\\BIN", b"EMPTY="],
        program_path: b"C:\\BIN\\PROG.COM",
        ..Default::default()
    };
    let expected = b"NAME=demo\0PATH=C:\\BIN\0EMPTY=\0\0\x01\0C:\\BIN\\PROG.COM\0";
    // Print the entire environment through the segment in PSP:002C.
    let program = [
        0xa1, 0x2c, 0, // MOV AX,[002C]
        0x8e, 0xd8, // MOV DS,AX
        0xba, 0, 0, // MOV DX,0
        0xbb, 1, 0, // MOV BX,1
        0xb9, 48, 0, // MOV CX,48: environment byte count
        0xb4, 0x40, 0xcd, 0x21, // Write the environment bytes.
        0xb8, 0, 0x4c, 0xcd, 0x21, // Exit 0.
    ];
    for mz in [false, true] {
        let image = if mz {
            common::mz_image(&program)
        } else {
            program.to_vec()
        };
        if mz {
            dos.launch_mz(&image, options).unwrap();
        } else {
            dos.launch_com(&image, options).unwrap();
        }
        let mut psp = [0; 256];
        dos.cpu().context().memory.read(0x10000, &mut psp).unwrap();
        let environment = u16::from_le_bytes([psp[0x2c], psp[0x2d]]);
        let program_end = u16::from_le_bytes([psp[2], psp[3]]);
        assert_eq!(environment, program_end + 1); // A distinct MCB follows the program.
        let mut mcb = [0; 5];
        dos.cpu()
            .context()
            .memory
            .read(u32::from(environment - 1) * 16, &mut mcb)
            .unwrap();
        assert_eq!(&mcb[1..3], &[0, 0x10]); // The root PSP owns the environment.
        assert_eq!(&mcb[3..5], &[3, 0]); // 48 bytes occupy three paragraphs.
        assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(0))
        );
        assert_eq!(common::output(dos.stdout()), expected);
    }

    // Reloading with default options replaces the old variables and path.
    dos.launch_com(&[0xc3], Default::default()).unwrap();
    let mut pointer = [0; 2];
    dos.cpu()
        .context()
        .memory
        .read(0x1002c, &mut pointer)
        .unwrap();
    let mut empty = [0; 5];
    dos.cpu()
        .context()
        .memory
        .read(u32::from(u16::from_le_bytes(pointer)) * 16, &mut empty)
        .unwrap();
    assert_eq!(empty, [0, 0, 1, 0, 0]);
}

async fn check_environment_validation() {
    let mut dos = common::dos().await;
    dos.launch_com(&[0xc3], Default::default()).unwrap();
    assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
    let saved_cpu = dos.cpu().context().cpu.snapshot();
    let mut before = vec![0; 0x90010]; // Root MCB, PSP, program and environment.
    dos.cpu()
        .context()
        .memory
        .read(0xfff0, &mut before)
        .unwrap();
    let too_large = vec![b'x'; 65532]; // Plus the two NULs, count and final NUL.
    for (options, error) in [
        (
            LoadOptions {
                environment: &[b"NO_EQUALS"],
                ..Default::default()
            },
            LoadError::InvalidEnvironment,
        ),
        (
            LoadOptions {
                environment: &[b"=VALUE"],
                ..Default::default()
            },
            LoadError::InvalidEnvironment,
        ),
        (
            LoadOptions {
                environment: &[b"A=B\0C"],
                ..Default::default()
            },
            LoadError::InvalidEnvironment,
        ),
        (
            LoadOptions {
                program_path: b"A\0B",
                ..Default::default()
            },
            LoadError::InvalidEnvironment,
        ),
        (
            LoadOptions {
                program_path: &too_large,
                ..Default::default()
            },
            LoadError::EnvironmentTooLarge,
        ),
    ] {
        assert_eq!(dos.launch_com(&[0xc3], options), Err(error.into()));
        assert_eq!(
            dos.launch_mz(&common::mz_image(&[0xc3]), options),
            Err(error.into())
        );
    }
    // This image fit without an environment, but its minimum now exceeds RAM.
    let mut image = common::mz_image(&[0xc3]);
    image[0x0a..0x0c].copy_from_slice(&0x8fef_u16.to_le_bytes());
    assert_eq!(
        dos.launch_mz(&image, Default::default()),
        Err(LoadError::InsufficientMemory.into())
    );
    let mut after = vec![0; before.len()];
    dos.cpu().context().memory.read(0xfff0, &mut after).unwrap();
    assert_eq!(after, before);
    assert_eq!(dos.cpu().context().cpu.snapshot(), saved_cpu);

    let maximum_path = vec![b'x'; 65531];
    dos.launch_com(
        &[0xc3],
        LoadOptions {
            program_path: &maximum_path,
            ..Default::default()
        },
    )
    .unwrap();
    let mut pointer = [0; 2];
    dos.cpu()
        .context()
        .memory
        .read(0x1002c, &mut pointer)
        .unwrap();
    let address = u32::from(u16::from_le_bytes(pointer)) * 16;
    let mut last = [0; 2];
    dos.cpu()
        .context()
        .memory
        .read(address + 65534, &mut last)
        .unwrap();
    assert_eq!(last, [b'x', 0]);
}

#[test]
fn com_and_mz_receive_environment_blocks() {
    pollster::block_on(check_environments());
}

#[test]
fn invalid_environments_preserve_the_current_program() {
    pollster::block_on(check_environment_validation());
}

#[test]
fn hardware_defaults_seed_the_launchers_environment_once() {
    pollster::block_on(async {
        let clock = relocade_clock_test_support::ManualClock::new();
        let (runner, _host, machine) = common::runner(clock.clock()).await;
        runner
            .run(std::future::pending::<()>(), async |runner| {
                let mut dos = relocade_dos::Dos::with_configuration(
                    runner,
                    relocade_dos::Configuration {
                        sound_card: relocade_dos::SoundCard::SoundBlaster2,
                    },
                )
                .await
                .unwrap();
                let mut environment = dos.initial_environment();
                for expected in [
                    b"BLASTER=A220 I7 D1 T3\0\0\x01\0\0".as_slice(),
                    b"\0\0\x01\0\0",
                ] {
                    let entries: Vec<_> = environment.iter().map(Vec::as_slice).collect();
                    dos.launch_com(
                        &[0xc3],
                        LoadOptions {
                            environment: &entries,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                    let psp = u32::from(dos.process().unwrap().psp_segment()) << 4;
                    let memory = dos.cpu().memory();
                    let segment = memory.view().read_u16(psp + 0x2c).unwrap();
                    let mut bytes = vec![0; expected.len()];
                    memory
                        .view()
                        .read(u32::from(segment) << 4, &mut bytes)
                        .unwrap();
                    assert_eq!(bytes, expected);
                    assert_eq!(dos.run().await, Ok(0));
                    // Removing BLASTER persists even though the card remains installed.
                    environment.clear();
                }
            })
            .await
            .unwrap();
        machine.shutdown().await.unwrap();
    });
}

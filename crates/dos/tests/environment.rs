mod common;

use relocade_dos::{LoadError, LoadOptions};
use relocade_x86::ExecutionExit;

async fn check_environments() {
    let mut dos = common::dos().await;
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
            dos.load_mz(&image, options).unwrap();
        } else {
            dos.load_com(&image, options).unwrap();
        }
        let mut psp = [0; 256];
        dos.runtime()
            .machine()
            .memory()
            .read(0x10000, &mut psp)
            .unwrap();
        let environment = u16::from_le_bytes([psp[0x2c], psp[0x2d]]);
        let program_end = u16::from_le_bytes([psp[2], psp[3]]);
        assert_eq!(environment, program_end + 1); // A distinct MCB follows the program.
        let mut mcb = [0; 5];
        dos.runtime()
            .machine()
            .memory()
            .read(u32::from(environment - 1) * 16, &mut mcb)
            .unwrap();
        assert_eq!(&mcb[1..3], &[0, 0x10]); // The root PSP owns the environment.
        assert_eq!(&mcb[3..5], &[3, 0]); // 48 bytes occupy three paragraphs.
        assert_eq!(dos.runtime().run(100).unwrap(), ExecutionExit::HostReturn);
        assert_eq!(dos.exit_status(), Some(Ok(0)));
        assert_eq!(dos.take_output(), expected);
    }

    // Reloading with default options replaces the old variables and path.
    dos.load_com(&[0xc3], Default::default()).unwrap();
    let mut pointer = [0; 2];
    dos.runtime()
        .machine()
        .memory()
        .read(0x1002c, &mut pointer)
        .unwrap();
    let mut empty = [0; 5];
    dos.runtime()
        .machine()
        .memory()
        .read(u32::from(u16::from_le_bytes(pointer)) * 16, &mut empty)
        .unwrap();
    assert_eq!(empty, [0, 0, 1, 0, 0]);
}

async fn check_environment_validation() {
    let mut dos = common::dos().await;
    dos.load_com(&[0xc3], Default::default()).unwrap();
    let saved_cpu = dos.runtime().machine().cpu().snapshot();
    let mut before = vec![0; 0x90010]; // Root MCB, PSP, program and environment.
    dos.runtime()
        .machine()
        .memory()
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
        assert_eq!(dos.load_com(&[0xc3], options), Err(error));
        assert_eq!(dos.load_mz(&common::mz_image(&[0xc3]), options), Err(error));
    }
    // This image fit without an environment, but its minimum now exceeds RAM.
    let mut image = common::mz_image(&[0xc3]);
    image[0x0a..0x0c].copy_from_slice(&0x8fef_u16.to_le_bytes());
    assert_eq!(
        dos.load_mz(&image, Default::default()),
        Err(LoadError::InsufficientMemory)
    );
    let mut after = vec![0; before.len()];
    dos.runtime()
        .machine()
        .memory()
        .read(0xfff0, &mut after)
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(dos.runtime().machine().cpu().snapshot(), saved_cpu);

    let maximum_path = vec![b'x'; 65531];
    dos.load_com(
        &[0xc3],
        LoadOptions {
            program_path: &maximum_path,
            ..Default::default()
        },
    )
    .unwrap();
    let mut pointer = [0; 2];
    dos.runtime()
        .machine()
        .memory()
        .read(0x1002c, &mut pointer)
        .unwrap();
    let address = u32::from(u16::from_le_bytes(pointer)) * 16;
    let mut last = [0; 2];
    dos.runtime()
        .machine()
        .memory()
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

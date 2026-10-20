use relocade_dos::{LoadError, ServiceError};
use relocade_x86::{ExecutionExit, Segment};

mod common;

// A COM image that prints a message and exits through DOS services.
const HELLO: &[u8] = &[
    0xba, 0x0c, 1, // MOV DX,010C
    0xb4, 9, // MOV AH,09
    0xcd, 0x21, // INT 21h: print DS:DX
    0xb8, 7, 0x4c, // MOV AX,4C07
    0xcd, 0x21, // INT 21h: exit 7
    b'H', b'e', b'l', b'l', b'o', b',', b' ', b'D', b'O', b'S', b'!', 13, 10, b'$',
];

async fn check_hello() {
    let mut dos = common::dos().await;
    dos.load_com(HELLO, Default::default()).unwrap();
    {
        let mut machine = dos.runtime().machine();
        for segment in [Segment::Cs, Segment::Ds, Segment::Es, Segment::Ss] {
            assert_eq!(machine.cpu().segment(segment).selector, 0x1000);
        }
        assert_eq!(machine.cpu().eip(), 0x100);
        assert_eq!(machine.cpu().sp(), 0xfffe);
        let mut psp = [0; 256];
        machine.memory().read(0x10000, &mut psp).unwrap();
        assert_eq!(&psp[..4], &[0xcd, 0x20, 0xfe, 0x9f]);
        assert_eq!(&psp[0x80..0x82], &[0, 13]);
        let mut word = [1; 2];
        machine.memory().read(0x1fffe, &mut word).unwrap();
        assert_eq!(word, [0, 0]);
    }
    assert_eq!(dos.exit_status(), None);
    assert_eq!(
        dos.runtime().run(1).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert!(common::output(dos.stdout()).is_empty()); // INT completed, before entering the hook.
    assert_eq!(
        dos.runtime().run(1).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(common::output(dos.stdout()), b"Hello, DOS!\r\n");
    assert_eq!(dos.runtime().machine().cpu().al(), b'$');
    assert_eq!(dos.runtime().machine().cpu().sp(), 0xfffe);
    assert_eq!(dos.runtime().machine().cpu().eip(), 0x107);
    assert_eq!(
        dos.runtime().run(u64::MAX).unwrap(),
        ExecutionExit::HostReturn
    );
    assert_eq!(dos.exit_status(), Some(Ok(7)));
    assert!(common::output(dos.stdout()).is_empty());
}

async fn check_com_bounds_and_return() {
    let mut image = vec![0x90; 0xfefe];
    image[0] = 0xc3; // RET pops the initial zero word and reaches PSP's INT 20h.
    image[0xfefd] = 0x41;
    let mut dos = common::dos().await;
    dos.load_com(&image, Default::default()).unwrap();
    let mut end = [0; 3];
    dos.runtime()
        .machine()
        .memory()
        .read(0x1fffd, &mut end)
        .unwrap();
    assert_eq!(end, [0x41, 0, 0]);
    assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(dos.exit_status(), Some(Ok(0)));
    assert_eq!(dos.runtime().machine().cpu().instruction_count(), 2);
    assert_eq!(
        dos.load_com(&[], Default::default()),
        Err(LoadError::EmptyImage)
    );
    image.push(0x90);
    assert_eq!(
        dos.load_com(&image, Default::default()),
        Err(LoadError::ImageTooLarge)
    );
    assert_eq!(dos.exit_status(), Some(Ok(0)));

    // Another program reuses the prepared environment and replaces its allocation.
    dos.load_com(&[0xc3], Default::default()).unwrap();
    assert_eq!(dos.exit_status(), None);
    dos.runtime()
        .machine()
        .memory()
        .read(0x1fffd, &mut end)
        .unwrap();
    assert_eq!(end, [0, 0, 0]);
    assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(dos.exit_status(), Some(Ok(0)));
}

async fn check_unsupported_service() {
    let mut dos = common::dos().await;
    dos.load_com(&[0xb4, 0xfe, 0xcd, 0x21], Default::default())
        .unwrap();
    assert_eq!(dos.runtime().run(10).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(
        dos.exit_status(),
        Some(Err(ServiceError::UnsupportedFunction(0xfe)))
    );
}

#[test]
fn com_prints_and_exits() {
    pollster::block_on(check_hello());
}

#[test]
fn com_reserves_stack_and_returns_through_psp() {
    pollster::block_on(check_com_bounds_and_return());
}

#[test]
fn unsupported_service_returns_to_host() {
    pollster::block_on(check_unsupported_service());
}

async fn check_unterminated_string() {
    let image = [
        0xb8, 0, 0x30, // MOV AX,3000
        0x8e, 0xd8, // MOV DS,AX
        0xba, 0xff, 0xff, // MOV DX,FFFF
        0xb4, 9, 0xcd, 0x21, // Print DS:DX
    ];
    let mut dos = common::dos().await;
    dos.load_com(&image, Default::default()).unwrap();
    dos.runtime()
        .machine()
        .memory()
        .write(0x3ffff, b"Z")
        .unwrap();
    dos.runtime()
        .machine()
        .memory()
        .write(0x30000, b"A")
        .unwrap();
    assert_eq!(
        dos.runtime().run(u64::MAX).unwrap(),
        ExecutionExit::HostReturn
    );
    assert_eq!(
        dos.exit_status(),
        Some(Err(ServiceError::UnterminatedString))
    );
    let output = common::output(dos.stdout());
    assert_eq!(output.len(), 65536);
    assert_eq!(&output[..2], b"ZA"); // The offset wraps within DS.
    assert!(output[2..].iter().all(|byte| *byte == 0));
}

#[test]
fn unterminated_string_wraps_offset_and_returns_to_host() {
    pollster::block_on(check_unterminated_string());
}

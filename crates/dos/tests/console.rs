mod common;
#[path = "console/screen.rs"]
mod screen;

use common::{path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;
use relocade_x86::ExecutionExit;
use std::io::{Read, Write};

#[test]
fn cooked_character_output_shares_tab_stops_with_strings_and_line_echo() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.cpu()
            .context()
            .memory()
            .write(0x10300, b"abc$")
            .unwrap();
        service(&mut dos, 0x0900, 0, 0, 0x300);
        assert_eq!(common::output(dos.stdout()), b"abc");
        dos.cpu().context().cpu().set_eflags(0x643);
        let cpu = service(&mut dos, 0x027b, 0x1234, 0x5678, u16::from(b'\t'));
        assert_eq!(cpu.registers.eax as u16, 0x0220);
        assert_eq!(cpu.flags.eflags(), 0x643);
        assert_eq!(cpu.registers.ebx as u16, 0x1234);
        assert_eq!(cpu.registers.ecx as u16, 0x5678);
        assert_eq!(common::output(dos.stdout()), b"     "); // Draining preserved the column.

        service(&mut dos, 0x0200, 0, 0, u16::from(b'\x08'));
        service(&mut dos, 0x0200, 0, 0, u16::from(b'\t'));
        assert_eq!(common::output(dos.stdout()), b"\x08 ");
        dos.cpu()
            .context()
            .memory()
            .write(0x10300, &[8, 0])
            .unwrap();
        dos.stdin().write_all(b"x\r").unwrap();
        service(&mut dos, 0x0a00, 0, 0, 0x300);
        assert_eq!(common::output(dos.stdout()), b"x\r");
        service(&mut dos, 0x0200, 0, 0, u16::from(b'\t'));
        assert_eq!(common::output(dos.stdout()), b"        "); // Echoed CR reset the column.

        dos.cpu()
            .context()
            .memory()
            .write(0x10300, b"\rabc")
            .unwrap();
        assert_eq!(success(&service(&mut dos, 0x4000, 1, 4, 0x300)), 4);
        service(&mut dos, 0x0600, 0, 0, u16::from(b'\t'));
        service(&mut dos, 0x0200, 0, 0, u16::from(b'\t'));
        assert_eq!(common::output(dos.stdout()), b"\rabc\t     ");

        dos.cpu()
            .context()
            .memory()
            .write(0x10300, b"\rabc\t\x1anot output")
            .unwrap();
        assert_eq!(success(&service(&mut dos, 0x4000, 2, 16, 0x300)), 5);
        assert_eq!(common::output(dos.stderr()), b"\rabc     ");
        assert!(common::output(dos.stdout()).is_empty());

        let file = success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0));
        success(&service(&mut dos, 0x4600, file, 1, 0));
        service(&mut dos, 0x0200, 0, 0, u16::from(b'\r'));
        for byte in [b'A', b'\t', 0xff] {
            service(&mut dos, 0x0200, 0, 0, u16::from(byte));
        }
        assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"\rA       \xff");
        assert!(common::output(dos.stdout()).is_empty());
    });
}

async fn check_handle_output() {
    let mut dos = common::dos().await;
    dos.load_com(&[0xcd, 0x21, 0xeb, 0xfe], Default::default())
        .unwrap(); // INT 21h; JMP $
    dos.cpu()
        .context()
        .memory()
        .write(0x10200, b"A$\0Z")
        .unwrap();
    // Deliberately start with the opposite carry value. IRET must restore all
    // other represented flags, including direction and interrupt enable.
    for (handle, count, carry_before, expected_ax, expected_carry, output) in [
        (1, 4, true, 4, false, b"A$\0Z".as_slice()),
        (2, 1, true, 1, false, b"A".as_slice()),
        (1, 0, true, 0, false, b"".as_slice()),
        (99, 4, false, 6, true, b"".as_slice()),
    ] {
        {
            let mut machine = dos.cpu().context();
            let mut cpu = machine.cpu();
            cpu.set_eip(0x100);
            cpu.set_ax(0x4000);
            cpu.set_bx(handle);
            cpu.set_cx(count);
            cpu.set_dx(0x200);
            cpu.set_eflags(0x642 | u32::from(carry_before));
        }
        assert_eq!(
            common::ready(dos.cpu().run(10)).unwrap(),
            ExecutionExit::BudgetExhausted
        );
        {
            let mut machine = dos.cpu().context();
            let cpu = machine.cpu();
            assert_eq!(cpu.ax(), expected_ax);
            assert_eq!(cpu.eflags(), 0x642 | u32::from(expected_carry));
            assert_eq!(cpu.sp(), 0xfffe);
            assert_eq!(cpu.eip(), 0x102);
        }
        if handle == 2 {
            assert_eq!(common::output(dos.stderr()), output);
            assert!(common::output(dos.stdout()).is_empty());
        } else {
            assert_eq!(common::output(dos.stdout()), output);
            assert!(common::output(dos.stderr()).is_empty());
        }
        assert_eq!(dos.exit_status(), None);
    }

    // The destination belongs to the open device, not to its handle number.
    success(&service(&mut dos, 0x4600, 2, 1, 0)); // Redirect stdout to stderr.
    success(&service(&mut dos, 0x4000, 1, 4, 0x200));
    assert!(common::output(dos.stdout()).is_empty());
    let mut prefix = [0; 2];
    dos.stderr().read_exact(&mut prefix).unwrap();
    assert_eq!(&prefix, b"A$");
    assert_eq!(common::output(dos.stderr()), b"\0Z");
    assert!(common::output(dos.stderr()).is_empty());
    success(&service(&mut dos, 0x4000, 2, 1, 0x200));
    assert_eq!(common::output(dos.stderr()), b"A"); // More output after draining.
}

async fn check_redirected_string() {
    let mut dos = common::dos().await;
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(success(&service(&mut dos, 0x4500, 1, 0, 0)), 5); // Save stdout.
    assert_eq!(success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0)), 6);
    success(&service(&mut dos, 0x4600, 6, 1, 0));
    dos.cpu()
        .context()
        .memory()
        .write(0x10200, b"out$not output")
        .unwrap();
    dos.cpu().context().cpu().set_eflags(0x0a57);
    let cpu = service(&mut dos, 0x0900, 0xabcd, 0x1234, 0x200);
    assert_eq!(cpu.registers.eax as u16, 0x0924); // AL='$'.
    assert_eq!(cpu.registers.ebx as u16, 0xabcd);
    assert_eq!(cpu.registers.ecx as u16, 0x1234);
    assert_eq!(cpu.flags.eflags(), 0x0a57); // String output preserves flags.
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"out");
    assert!(common::output(dos.stdout()).is_empty());
    assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 3);

    // An empty string must not become AH=40h's zero-byte truncation operation.
    success(&service(&mut dos, 0x4200, 6, 0, 0));
    service(&mut dos, 0x0900, 0, 0, 0x203);
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"out");
    assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 0);

    success(&service(&mut dos, 0x4600, 5, 1, 0)); // Restore console output.
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    service(&mut dos, 0x0900, 0, 0, 0x200);
    assert_eq!(common::output(dos.stdout()), b"out");
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"out");
    service(&mut dos, 0x0900, 0, 0, 0x200);
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert!(common::output(dos.stdout()).is_empty());
}

async fn check_direct_console_io() {
    let mut dos = common::dos().await;
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    dos.cpu().context().cpu().set_eflags(0x603);
    let cpu = service(&mut dos, 0x06aa, 0, 0, 0xff);
    assert_eq!(cpu.registers.eax as u16, 0x0600);
    assert_eq!(cpu.flags.eflags(), 0x643); // Empty input sets ZF, preserving CF/IF/DF.

    // Raw input includes the extended-key prefix, Ctrl-C, Ctrl-Z and FFh.
    dos.stdin().write_all(b"A\0").unwrap();
    dos.stdin().write_all(&[0x48, 3, 0x1a, 0xff]).unwrap();
    for byte in [b'A', 0, 0x48, 3, 0x1a, 0xff] {
        dos.cpu().context().cpu().set_eflags(0x643);
        let cpu = service(&mut dos, 0x06aa, 0x1234, 0x5678, 0xff);
        assert_eq!(cpu.registers.eax as u16, 0x0600 | u16::from(byte));
        assert_eq!(cpu.flags.eflags(), 0x603); // Even a zero byte is available input.
        assert_eq!(cpu.registers.ebx as u16, 0x1234);
        assert_eq!(cpu.registers.ecx as u16, 0x5678);
        assert_eq!(cpu.registers.esp as u16, 0xfffe);
    }
    let cpu = service(&mut dos, 0x06aa, 0, 0, 0xff);
    assert_eq!(cpu.registers.eax as u16, 0x0600);
    assert_eq!(cpu.flags.eflags(), 0x643);
    assert!(common::output(dos.stdout()).is_empty()); // No input echo.

    let cpu = service(&mut dos, 0x06aa, 0, 0, u16::from(b'!'));
    assert_eq!(cpu.registers.eax as u16, 0x0621);
    assert_eq!(cpu.flags.eflags(), 0x643); // Output preserves flags.
    assert_eq!(common::output(dos.stdout()), b"!");
    assert!(common::output(dos.stderr()).is_empty());

    dos.stdin().write_all(b"old input").unwrap();
    service(&mut dos, 0x0600, 0, 0, u16::from(b'X'));
    success(&service(&mut dos, 0x4600, 2, 1, 0));
    service(&mut dos, 0x0600, 0, 0, u16::from(b'Y'));
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert!(common::output(dos.stdout()).is_empty());
    assert!(common::output(dos.stderr()).is_empty());
    let cpu = service(&mut dos, 0x06aa, 0, 0, 0xff);
    assert_eq!(cpu.registers.eax as u16, 0x0600);
    assert_ne!(cpu.flags.eflags() & 0x40, 0); // New program gets empty stdin.
}

async fn check_redirected_character_io() {
    let mut dos = common::dos().await;
    dos.mount_file("IN.TXT", b"F\x1a".to_vec(), FileAccess::ReadOnly)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    dos.stdin().write_all(b"K").unwrap();
    assert_eq!(success(&service(&mut dos, 0x4500, 0, 0, 0)), 5); // Save stdin.
    assert_eq!(success(&path_service(&mut dos, b"IN.TXT", 0x3d00, 0)), 6);
    success(&service(&mut dos, 0x4600, 6, 0, 0));
    for byte in [b'F', 0x1a] {
        let cpu = service(&mut dos, 0x0600, 0, 0, 0xff);
        assert_eq!(cpu.registers.eax as u16, 0x0600 | u16::from(byte));
        assert_eq!(cpu.flags.eflags() & 0x40, 0);
    }
    let cpu = service(&mut dos, 0x06aa, 0, 0, 0xff);
    assert_eq!(cpu.registers.eax as u16, 0x0600);
    assert_ne!(cpu.flags.eflags() & 0x40, 0); // EOF does not fall back to keyboard input.
    assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 2); // Shared cursor.
    success(&service(&mut dos, 0x4600, 5, 0, 0));
    assert_eq!(
        service(&mut dos, 0x0600, 0, 0, 0xff).registers.eax as u16,
        0x064b
    );

    assert_eq!(success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0)), 7);
    success(&service(&mut dos, 0x4600, 7, 1, 0));
    service(&mut dos, 0x0600, 0, 0, u16::from(b'O'));
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"O");
    assert!(common::output(dos.stdout()).is_empty());
}

#[test]
fn string_output_follows_stdout_redirection() {
    pollster::block_on(check_redirected_string());
}

#[test]
fn handle_output_returns_count_and_errors_through_iret() {
    pollster::block_on(check_handle_output());
}

#[test]
fn direct_console_io_polls_raw_input_and_writes_output() {
    pollster::block_on(check_direct_console_io());
}

#[test]
fn direct_character_io_follows_handle_redirection() {
    pollster::block_on(check_redirected_character_io());
}

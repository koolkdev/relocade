mod common;

use common::{path_service, service, success, SERVICE_PROGRAM};
use relocade_x86::ExecutionExit;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_handle_output() {
    let mut dos = common::dos().await;
    dos.load_com(&[0xcd, 0x21, 0xeb, 0xfe], Default::default())
        .unwrap(); // INT 21h; JMP $
    dos.runtime()
        .machine()
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
            let mut machine = dos.runtime().machine();
            let mut cpu = machine.cpu();
            cpu.set_eip(0x100);
            cpu.set_ax(0x4000);
            cpu.set_bx(handle);
            cpu.set_cx(count);
            cpu.set_dx(0x200);
            cpu.set_eflags(0x642 | u32::from(carry_before));
        }
        assert_eq!(
            dos.runtime().run(10).unwrap(),
            ExecutionExit::BudgetExhausted
        );
        let mut machine = dos.runtime().machine();
        let cpu = machine.cpu();
        assert_eq!(cpu.ax(), expected_ax);
        assert_eq!(cpu.eflags(), 0x642 | u32::from(expected_carry));
        assert_eq!(cpu.sp(), 0xfffe);
        assert_eq!(cpu.eip(), 0x102);
        assert_eq!(dos.take_output(), output);
        assert_eq!(dos.exit_status(), None);
    }
}

async fn check_redirected_string() {
    let mut dos = common::dos().await;
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(success(&service(&mut dos, 0x4500, 1, 0, 0)), 5); // Save stdout.
    assert_eq!(success(&path_service(&mut dos, b"OUT.TXT", 0x3c00, 0)), 6);
    success(&service(&mut dos, 0x4600, 6, 1, 0));
    dos.runtime()
        .machine()
        .memory()
        .write(0x10200, b"out$not output")
        .unwrap();
    dos.runtime().machine().cpu().set_eflags(0x0a57);
    let cpu = service(&mut dos, 0x0900, 0xabcd, 0x1234, 0x200);
    assert_eq!(cpu.registers.eax as u16, 0x0924); // AL='$'.
    assert_eq!(cpu.registers.ebx as u16, 0xabcd);
    assert_eq!(cpu.registers.ecx as u16, 0x1234);
    assert_eq!(cpu.flags.eflags(), 0x0a57); // String output preserves flags.
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"out");
    assert!(dos.take_output().is_empty());
    assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 3);

    // An empty string must not become AH=40h's zero-byte truncation operation.
    success(&service(&mut dos, 0x4200, 6, 0, 0));
    service(&mut dos, 0x0900, 0, 0, 0x203);
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"out");
    assert_eq!(success(&service(&mut dos, 0x4201, 6, 0, 0)), 0);

    success(&service(&mut dos, 0x4600, 5, 1, 0)); // Restore console output.
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    service(&mut dos, 0x0900, 0, 0, 0x200);
    assert_eq!(dos.take_output(), b"out");
    assert_eq!(dos.read_file("OUT.TXT").unwrap(), b"out");
    service(&mut dos, 0x0900, 0, 0, 0x200);
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert!(dos.take_output().is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn string_output_follows_stdout_redirection() {
    pollster::block_on(check_redirected_string());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn string_output_follows_stdout_redirection() {
    check_redirected_string().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn handle_output_returns_count_and_errors_through_iret() {
    pollster::block_on(check_handle_output());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn handle_output_returns_count_and_errors_through_iret() {
    check_handle_output().await;
}

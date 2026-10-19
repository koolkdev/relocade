mod common;

use wasm86_x86::ExecutionExit;

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

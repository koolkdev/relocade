mod common;

use common::Backend;
use wasm86_dos::{Dos, DosState, MountError};
use wasm86_x86::{CpuState, ExecutionExit};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_file_program() {
    let mut dos = common::dos().await;
    dos.mount_file(r"C:\DATA\HELLO.TXT", b"file\r\n".to_vec())
        .unwrap();
    let mut program = vec![
        0xb8, 0, 0x3d, // MOV AX,3D00: open read-only
        0xba, 0x40, 1, // MOV DX,0140: pathname
        0xcd, 0x21, 0x72, 0x24, // JC failure
        0x89, 0xc3, // MOV BX,AX: file handle
        0x89, 0xde, // MOV SI,BX: preserve it for close
        0xb9, 0x40, 0, // MOV CX,64: read capacity
        0xba, 0, 2, // MOV DX,0200: buffer
        0xb4, 0x3f, 0xcd, 0x21, // Read file.
        0x72, 0x14, // JC failure
        0x89, 0xc1, // MOV CX,AX: bytes actually read
        0xbb, 1, 0, // MOV BX,1: stdout
        0xb4, 0x40, 0xcd, 0x21, // Write buffer.
        0x89, 0xf3, // MOV BX,SI: original handle
        0xb4, 0x3e, 0xcd, 0x21, // Close file.
        0xb8, 0, 0x4c, 0xcd, 0x21, // Exit 0.
        0xb8, 0xff, 0x4c, 0xcd, 0x21, // failure: exit 255.
    ];
    program.resize(0x40, 0);
    program.extend_from_slice(b"data/hello.txt\0");
    // Program replacement keeps mounts and resets the program's file handles.
    for _ in 0..2 {
        dos.load_com(&program).unwrap();
        assert_eq!(dos.runtime().run(100).unwrap(), ExecutionExit::HostReturn);
        assert_eq!(dos.exit_status(), Some(Ok(0)));
        assert_eq!(dos.take_output(), b"file\r\n");
    }
}

// INT 21h followed by JMP $ lets each host assertion inspect the IRET result.
const SERVICE_PROGRAM: &[u8] = &[0xcd, 0x21, 0xeb, 0xfe];

fn service(dos: &mut Dos<Backend<DosState>>, ax: u16, bx: u16, cx: u16, dx: u16) -> CpuState {
    {
        let mut machine = dos.runtime().machine();
        let mut cpu = machine.cpu();
        cpu.set_eip(0x100);
        cpu.set_ax(ax);
        cpu.set_bx(bx);
        cpu.set_cx(cx);
        cpu.set_dx(dx);
    }
    assert_eq!(
        dos.runtime().run(10).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    dos.runtime().machine().cpu().snapshot()
}

fn open(dos: &mut Dos<Backend<DosState>>, path: &[u8], mode: u8) -> CpuState {
    let mut terminated = path.to_vec();
    terminated.push(0);
    dos.runtime()
        .machine()
        .memory()
        .write(0x10300, &terminated)
        .unwrap();
    service(dos, 0x3d00 | u16::from(mode), 0, 0, 0x300)
}

fn success(cpu: &CpuState) -> u16 {
    assert_eq!(cpu.flags.eflags() & 1, 0);
    cpu.registers.eax as u16
}

fn error(cpu: &CpuState, code: u16) {
    assert_eq!(cpu.flags.eflags() & 1, 1);
    assert_eq!(cpu.registers.eax as u16, code);
}

fn buffer(dos: &mut Dos<Backend<DosState>>) -> [u8; 8] {
    let mut bytes = [0; 8];
    dos.runtime()
        .machine()
        .memory()
        .read(0x10200, &mut bytes)
        .unwrap();
    bytes
}

async fn check_handles() {
    let mut dos = common::dos().await;
    for path in ["", "C:\\", r"D:\X.BIN", "X?.BIN", "X\0.BIN", "é.bin"] {
        assert_eq!(dos.mount_file(path, vec![]), Err(MountError::InvalidPath));
    }
    dos.mount_file("X.BIN", b"abcd".to_vec()).unwrap();
    dos.load_com(SERVICE_PROGRAM).unwrap();
    error(&open(&mut dos, b"MISSING.BIN", 0), 2);
    error(&open(&mut dos, b"MISSING/X.BIN", 0), 3);
    error(&open(&mut dos, b"X.BIN", 2), 5); // Read/write mode on a read-only mount.
    error(&open(&mut dos, b"X.BIN", 3), 12); // Invalid access mode.
    error(&open(&mut dos, b"X.BIN", 0x50), 12); // Invalid sharing mode.
    assert_eq!(success(&open(&mut dos, b"c:/./x.bin", 0)), 5);
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0x40)), 6); // Deny-none sharing.

    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 2, 0x200)), 2);
    assert_eq!(&buffer(&mut dos)[..2], b"ab");
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 1, 0x200)), 1);
    assert_eq!(buffer(&mut dos)[0], b'a'); // Independent cursor.
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 0, 0x200)), 0);
    dos.runtime()
        .machine()
        .memory()
        .write(0x10200, b"!!!!!!!!")
        .unwrap();
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 8, 0x200)), 2);
    assert_eq!(buffer(&mut dos), *b"cd!!!!!!"); // Short read preserves the buffer tail.
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 8, 0x200)), 0); // EOF.
    error(&service(&mut dos, 0x4000, 5, 1, 0x200), 5);
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    error(&service(&mut dos, 0x3e00, 5, 0, 0), 6);
    error(&service(&mut dos, 0x3f00, 5, 0, 0x200), 6);
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 5); // Reuse a closed slot.

    dos.mount_file("X.BIN", b"new".to_vec()).unwrap();
    assert_eq!(success(&service(&mut dos, 0x3f00, 6, 8, 0x200)), 3);
    assert_eq!(&buffer(&mut dos)[..3], b"bcd"); // An existing handle keeps its bytes.
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 7);
    assert_eq!(success(&service(&mut dos, 0x3f00, 7, 8, 0x200)), 3);
    assert_eq!(&buffer(&mut dos)[..3], b"new");

    dos.load_com(SERVICE_PROGRAM).unwrap();
    error(&service(&mut dos, 0x3f00, 6, 1, 0x200), 6); // Old program's handle closed.
    for handle in 5..20 {
        assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), handle);
    }
    error(&open(&mut dos, b"X.BIN", 0), 4);
    success(&service(&mut dos, 0x3e00, 10, 0, 0));
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 10);

    dos.load_com(SERVICE_PROGRAM).unwrap();
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0x10)), 5); // Deny other reads.
    error(&open(&mut dos, b"X.BIN", 0), 5);
    success(&service(&mut dos, 0x3e00, 5, 0, 0));
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 5);
}

async fn check_standard_handles() {
    let mut dos = common::dos().await;
    dos.mount_file("X.BIN", b"file".to_vec()).unwrap();
    dos.load_com(SERVICE_PROGRAM).unwrap();

    success(&service(&mut dos, 0x3e00, 1, 0, 0)); // Close stdout.
    error(&service(&mut dos, 0x3e00, 1, 0, 0), 6);
    error(&service(&mut dos, 0x4000, 1, 1, 0x200), 6);
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 1);
    assert_eq!(success(&service(&mut dos, 0x3f00, 1, 4, 0x200)), 4);
    assert_eq!(&buffer(&mut dos)[..4], b"file");
    error(&service(&mut dos, 0x4000, 1, 4, 0x200), 5); // Slot 1 now holds a read-only file.
    assert_eq!(dos.take_output(), b"");
    assert_eq!(success(&service(&mut dos, 0x4000, 2, 4, 0x200)), 4);
    assert_eq!(dos.take_output(), b"file"); // Closing stdout leaves stderr open.

    for handle in 0..5 {
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    }
    error(&service(&mut dos, 0x4000, 2, 1, 0x200), 6);
    for handle in 0..20 {
        assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), handle);
    }
    error(&open(&mut dos, b"X.BIN", 0), 4);

    dos.load_com(SERVICE_PROGRAM).unwrap();
    assert_eq!(success(&open(&mut dos, b"X.BIN", 0)), 5);
    assert_eq!(success(&service(&mut dos, 0x3f00, 5, 4, 0x200)), 4);
    assert_eq!(success(&service(&mut dos, 0x4000, 1, 4, 0x200)), 4);
    assert_eq!(dos.take_output(), b"file"); // New program gets fresh standard handles.
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn standard_handles_can_be_closed_reused_and_reset() {
    pollster::block_on(check_standard_handles());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn standard_handles_can_be_closed_reused_and_reset() {
    check_standard_handles().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn program_reads_a_mounted_file_and_prints_it() {
    pollster::block_on(check_file_program());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn program_reads_a_mounted_file_and_prints_it() {
    check_file_program().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn file_handles_report_counts_errors_and_reset_between_programs() {
    pollster::block_on(check_handles());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn file_handles_report_counts_errors_and_reset_between_programs() {
    check_handles().await;
}

mod common;

use common::{error, path_service, service, success, Backend, SERVICE_PROGRAM};
use relocade_dos::{Dos, DosState, FileAccess};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn attributes(dos: &mut Dos<Backend<DosState>>, path: &[u8]) -> u16 {
    let cpu = path_service(dos, path, 0x4300, 0xffff);
    success(&cpu);
    cpu.registers.ecx as u16
}

fn found_attributes(dos: &mut Dos<Backend<DosState>>, path: &[u8], mask: u16) -> u8 {
    success(&path_service(dos, path, 0x4e00, mask));
    let mut byte = [0];
    dos.runtime()
        .machine()
        .memory()
        .read(0x10080 + 21, &mut byte)
        .unwrap();
    byte[0]
}

async fn check_file_attributes() {
    let mut dos = common::dos().await;
    dos.mount_file("DATA/FILE.BIN", b"ab".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    dos.mount_file("LOCKED.DAT", vec![], FileAccess::ReadOnly)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    dos.runtime().machine().cpu().set_eflags(0x0a57);
    assert_eq!(attributes(&mut dos, b"LOCKED.DAT"), 0x21);
    assert_eq!(dos.runtime().machine().cpu().eflags(), 0x0a56);
    assert_eq!(attributes(&mut dos, b"C:\\"), 0x10);
    success(&path_service(&mut dos, b"DATA", 0x3b00, 0));
    assert_eq!(attributes(&mut dos, b"file.bin"), 0x20);
    assert_eq!(success(&path_service(&mut dos, b"FILE.BIN", 0x3d42, 0)), 5);

    // Set read-only, hidden and system; archive can be cleared independently.
    success(&path_service(&mut dos, b"C:file.bin", 0x4301, 0x07));
    assert_eq!(attributes(&mut dos, b"FILE.BIN"), 0x07);
    error(&path_service(&mut dos, b"FILE.BIN", 0x4e00, 0), 18);
    assert_eq!(found_attributes(&mut dos, b"FILE.BIN", 0x06), 0x07);
    error(&path_service(&mut dos, b"FILE.BIN", 0x3d42, 0), 5);
    assert_eq!(success(&path_service(&mut dos, b"FILE.BIN", 0x3d40, 0)), 6);

    // Permissions change future opens; existing read/write handles keep their rights.
    dos.runtime()
        .machine()
        .memory()
        .write(0x10400, b"x")
        .unwrap();
    assert_eq!(success(&service(&mut dos, 0x4000, 5, 1, 0x400)), 1);
    error(&service(&mut dos, 0x4000, 6, 1, 0x400), 5);
    success(&path_service(&mut dos, b"FILE.BIN", 0x4301, 0x06));
    assert_eq!(attributes(&mut dos, b"FILE.BIN"), 0x06);
    error(&service(&mut dos, 0x4000, 6, 1, 0x400), 5);
    assert_eq!(success(&path_service(&mut dos, b"FILE.BIN", 0x3d42, 0)), 7);
    assert_eq!(dos.read_file("DATA/FILE.BIN").unwrap(), b"xb");

    // A mounted read-only file can be made writable through the DOS attribute API.
    success(&path_service(&mut dos, b"\\LOCKED.DAT", 0x4301, 0));
    assert_eq!(
        success(&path_service(&mut dos, b"\\LOCKED.DAT", 0x3d42, 0)),
        8
    );
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(attributes(&mut dos, b"FILE.BIN") & !0x20, 0x06);
    dos.mount_file("DATA/FILE.BIN", vec![], FileAccess::ReadOnly)
        .unwrap();
    assert_eq!(attributes(&mut dos, b"FILE.BIN"), 0x21);
}

async fn check_directory_attributes_and_errors() {
    let mut dos = common::dos().await;
    dos.mount_file("DATA/FILE.BIN", vec![], FileAccess::ReadWrite)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    success(&path_service(&mut dos, b"DATA", 0x4301, 0x13));
    assert_eq!(attributes(&mut dos, b"DATA"), 0x13);
    error(&path_service(&mut dos, b"DATA", 0x4e00, 0x10), 18);
    assert_eq!(found_attributes(&mut dos, b"DATA", 0x12), 0x13);
    success(&path_service(&mut dos, b"DATA", 0x4301, 0));
    assert_eq!(attributes(&mut dos, b"DATA"), 0x10); // The directory bit cannot be cleared.

    for mask in [0x08, 0x10, 0x40, 0x100] {
        error(&path_service(&mut dos, b"DATA/FILE.BIN", 0x4301, mask), 5);
        assert_eq!(attributes(&mut dos, b"DATA/FILE.BIN"), 0x20);
    }
    error(&path_service(&mut dos, b"\\", 0x4301, 0), 5);
    for operation in [0x4300, 0x4301] {
        error(&path_service(&mut dos, b"DATA/MISSING", operation, 0), 2);
        error(&path_service(&mut dos, b"MISSING/FILE", operation, 0), 3);
        error(
            &path_service(&mut dos, b"DATA/FILE.BIN/CHILD", operation, 0),
            3,
        );
        error(&path_service(&mut dos, b"D:\\FILE", operation, 0), 3);
        error(&path_service(&mut dos, b"DATA/*", operation, 0), 3);
    }
    error(&path_service(&mut dos, b"DATA/FILE.BIN", 0x4302, 0), 1);
    assert_eq!(dos.exit_status(), None);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn attributes_control_future_opens_and_match_search_results() {
    pollster::block_on(check_file_attributes());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn attributes_control_future_opens_and_match_search_results() {
    check_file_attributes().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn directory_attributes_preserve_entry_type_and_report_path_errors() {
    pollster::block_on(check_directory_attributes_and_errors());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn directory_attributes_preserve_entry_type_and_report_path_errors() {
    check_directory_attributes_and_errors().await;
}

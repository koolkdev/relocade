mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};

fn attributes(dos: &mut Dos, path: &[u8]) -> u16 {
    let cpu = path_service(dos, path, 0x4300, 0xffff);
    success(&cpu);
    cpu.registers.ecx as u16
}

fn found_attributes(dos: &mut Dos, path: &[u8], mask: u16) -> u8 {
    success(&path_service(dos, path, 0x4e00, mask));
    let mut byte = [0];
    dos.cpu()
        .context()
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
    dos.cpu().context().cpu().set_eflags(0x0a57);
    assert_eq!(attributes(&mut dos, b"LOCKED.DAT"), 0x21);
    assert_eq!(dos.cpu().context().cpu().eflags(), 0x0a56);
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
    dos.cpu().context().memory().write(0x10400, b"x").unwrap();
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
    assert_eq!(attributes(&mut dos, b"FILE.BIN"), 0x26);
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

async fn check_archive_on_close() {
    let mut dos = common::dos().await;
    dos.mount_file("DATA.BIN", b"abc".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    dos.cpu().context().memory().write(0x10400, b"x").unwrap();
    success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0x06));

    // A failed write must not make an otherwise unmodified handle dirty.
    let handle = success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0));
    success(&service(&mut dos, 0x4200, handle, 0xffff, 0xffff));
    error(&service(&mut dos, 0x4000, handle, 1, 0x400), 5);
    success(&service(&mut dos, 0x3e00, handle, 0, 0));
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x06);
    assert_eq!(dos.read_file("DATA.BIN").unwrap(), b"abc");

    let handle = success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0));
    let duplicate = success(&service(&mut dos, 0x4500, handle, 0, 0));
    let reader = success(&path_service(&mut dos, b"DATA.BIN", 0x3d40, 0));
    success(&service(&mut dos, 0x4000, handle, 1, 0x400));
    success(&service(&mut dos, 0x3e00, reader, 0, 0)); // A separate open has no writes.
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x06);
    success(&service(&mut dos, 0x3e00, handle, 0, 0));
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x26);
    assert_eq!(found_attributes(&mut dos, b"DATA.BIN", 0x06), 0x26);
    success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0x06));
    success(&service(&mut dos, 0x4600, duplicate, duplicate, 0));
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x06);
    assert_eq!(found_attributes(&mut dos, b"DATA.BIN", 0x06), 0x06);
    success(&service(&mut dos, 0x3e00, duplicate, 0, 0));
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x26);
    assert_eq!(found_attributes(&mut dos, b"DATA.BIN", 0x06), 0x26);

    // CX=0 truncates or extends the file at its cursor, and counts as a write.
    for length in [1, 4] {
        success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0x06));
        let handle = success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0));
        success(&service(&mut dos, 0x4200, handle, 0, length));
        success(&service(&mut dos, 0x4000, handle, 0, 0x400));
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x26);
        assert_eq!(
            dos.read_file("DATA.BIN").unwrap().len(),
            usize::from(length)
        );
    }
}

async fn check_archive_cleanup_and_replacement() {
    let mut dos = common::dos().await;
    dos.mount_file("DATA.BIN", b"abc".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0));
    let handle = success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0));
    let duplicate = success(&service(&mut dos, 0x4500, handle, 0, 0));
    success(&service(&mut dos, 0x4000, handle, 0, 0));
    success(&service(&mut dos, 0x4600, 1, handle, 0));
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x20);
    success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0));
    success(&service(&mut dos, 0x4600, 1, duplicate, 0));
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x20);

    // An old open file must not update a different file mounted at the same path.
    let old = success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0));
    success(&service(&mut dos, 0x4000, old, 0, 0));
    dos.mount_file("DATA.BIN", b"replacement".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0));
    success(&service(&mut dos, 0x3e00, old, 0, 0));
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0);
    assert_eq!(dos.read_file("DATA.BIN").unwrap(), b"replacement");

    // Reloading closes every handle through the same process-cleanup path.
    let handle = success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0));
    success(&service(&mut dos, 0x4500, handle, 0, 0));
    success(&service(&mut dos, 0x4000, handle, 0, 0));
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x20);
    success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0));
    success(&path_service(&mut dos, b"DATA.BIN", 0x3d40, 0)); // Unmodified open.
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0);
}

#[test]
fn attributes_control_future_opens_and_match_search_results() {
    pollster::block_on(check_file_attributes());
}

#[test]
fn directory_attributes_preserve_entry_type_and_report_path_errors() {
    pollster::block_on(check_directory_attributes_and_errors());
}

#[test]
fn archive_is_set_when_written_handles_close() {
    pollster::block_on(check_archive_on_close());
}

#[test]
fn archive_updates_follow_handle_replacement_and_process_cleanup() {
    pollster::block_on(check_archive_cleanup_and_replacement());
}

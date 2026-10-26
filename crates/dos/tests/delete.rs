mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;

async fn check_deletion() {
    let mut dos = common::dos().await;
    dos.mount_file("DATA/FILE.BIN", b"keep".to_vec(), FileAccess::ReadOnly)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    success(&path_service(&mut dos, b"DATA", 0x3b00, 0));
    error(&path_service(&mut dos, b"FILE.BIN", 0x4100, 0), 5);
    assert_eq!(dos.read_file("DATA/FILE.BIN").unwrap(), b"keep");
    success(&path_service(&mut dos, b"FILE.BIN", 0x4301, 0x06)); // Hidden/system is deletable.
    let handle = success(&path_service(&mut dos, b"FILE.BIN", 0x3d40, 0));
    let duplicate = success(&service(&mut dos, 0x4500, handle, 0, 0));
    error(&path_service(&mut dos, b"FILE.BIN", 0x4100, 0), 5);
    success(&service(&mut dos, 0x3e00, handle, 0, 0));
    error(&path_service(&mut dos, b"FILE.BIN", 0x4100, 0), 5);
    success(&service(&mut dos, 0x3e00, duplicate, 0, 0));
    dos.cpu().context().cpu.set_eflags(0x0a57);
    let cpu = path_service(&mut dos, b"c:file.bin", 0x4100, 0);
    success(&cpu);
    assert_eq!(cpu.flags.eflags(), 0x0a56);
    assert_eq!(dos.read_file("DATA/FILE.BIN"), None);
    error(&path_service(&mut dos, b"FILE.BIN", 0x4300, 0), 2);
    error(&path_service(&mut dos, b"FILE.BIN", 0x4e00, 0x06), 18);

    // Recreating the same name must not inherit its old hidden/system attributes.
    let handle = success(&path_service(&mut dos, b"FILE.BIN", 0x3c00, 0));
    success(&service(&mut dos, 0x3e00, handle, 0, 0));
    let cpu = path_service(&mut dos, b"FILE.BIN", 0x4300, 0);
    success(&cpu);
    assert_eq!(cpu.registers.ecx as u16, 0x20);
    success(&path_service(&mut dos, b"FILE.BIN", 0x4e00, 0));
    for (path, code) in [
        (b"MISSING".as_slice(), 2),
        (b"MISSING/FILE", 3),
        (b"FILE.BIN/CHILD", 3),
        (b"D:\\FILE", 3),
        (b"*.BIN", 3),
        (b"\\DATA", 5),
    ] {
        error(&path_service(&mut dos, path, 0x4100, 0), code);
    }
    assert_eq!(dos.read_file("DATA/FILE.BIN"), Some(vec![]));

    // Open handles refer to file identity, not a path later replaced by the host.
    let old = success(&path_service(&mut dos, b"FILE.BIN", 0x3d40, 0));
    dos.mount_file(
        "DATA/FILE.BIN",
        b"replacement".to_vec(),
        FileAccess::ReadWrite,
    )
    .unwrap();
    success(&path_service(&mut dos, b"\\DATA\\FILE.BIN", 0x4100, 0));
    assert_eq!(success(&service(&mut dos, 0x3f00, old, 1, 0x400)), 0);
    success(&service(&mut dos, 0x3e00, old, 0, 0));
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(dos.read_file("DATA/FILE.BIN"), None);
}

#[test]
fn deletion_checks_attributes_and_open_file_identity_before_removing_a_name() {
    pollster::block_on(check_deletion());
}

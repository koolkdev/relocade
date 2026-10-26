mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};

fn current_directory(dos: &mut Dos, drive: u16) -> Vec<u8> {
    dos.cpu().context().cpu().set_si(0x400);
    dos.cpu()
        .context()
        .memory()
        .write(0x10400, &[0xff; 64])
        .unwrap();
    assert_eq!(success(&service(dos, 0x4700, 0, 0, drive)), 0x0100);
    let mut buffer = [0; 64];
    dos.cpu()
        .context()
        .memory()
        .read(0x10400, &mut buffer)
        .unwrap();
    let end = buffer.iter().position(|byte| *byte == 0).unwrap();
    assert!(buffer[end + 1..].iter().all(|byte| *byte == 0xff));
    buffer[..end].to_vec()
}

async fn check_directory_paths() {
    let mut dos = common::dos().await;
    for (path, data) in [
        ("DATA/NESTED/X.DAT", b"nested".as_slice()),
        ("X.DAT", b"root"),
    ] {
        dos.mount_file(path, data.to_vec(), FileAccess::ReadOnly)
            .unwrap();
    }
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(current_directory(&mut dos, 0), b"");
    success(&path_service(&mut dos, b"c:\\data/nested", 0x3b00, 0));
    assert_eq!(current_directory(&mut dos, 3), b"DATA\\NESTED");

    for (path, expected) in [
        (b"X.DAT".as_slice(), b"nested".as_slice()),
        (b"C:X.DAT", b"nested"),
        (b"C:\\X.DAT", b"root"),
        (b"\\X.DAT", b"root"),
    ] {
        let handle = success(&path_service(&mut dos, path, 0x3d00, 0));
        assert_eq!(
            success(&service(&mut dos, 0x3f00, handle, 8, 0x400)),
            expected.len() as u16
        );
        let mut bytes = vec![0; expected.len()];
        dos.cpu()
            .context()
            .memory()
            .read(0x10400, &mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    }
    // Guest creates follow the current directory; host paths remain root-relative.
    let handle = success(&path_service(&mut dos, b"NEW.DAT", 0x3c00, 0));
    success(&service(&mut dos, 0x3e00, handle, 0, 0));
    assert_eq!(dos.read_file("DATA/NESTED/NEW.DAT"), Some(vec![]));
    assert_eq!(dos.read_file("NEW.DAT"), None);
    dos.mount_file("HOST.DAT", vec![], FileAccess::ReadOnly)
        .unwrap();
    assert_eq!(dos.read_file("HOST.DAT"), Some(vec![]));

    for path in [b"MISSING".as_slice(), b"X.DAT", b"D:\\", b"*"] {
        error(&path_service(&mut dos, path, 0x3b00, 0), 3);
    }
    assert_eq!(current_directory(&mut dos, 0), b"DATA\\NESTED");
    // Current directories belong to the DOS environment, not to an open program.
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(current_directory(&mut dos, 0), b"DATA\\NESTED");
    success(&path_service(&mut dos, b"..", 0x3b00, 0));
    assert_eq!(current_directory(&mut dos, 0), b"DATA");
    success(&path_service(&mut dos, b".\\", 0x3b00, 0));
    assert_eq!(current_directory(&mut dos, 0), b"DATA");
    success(&path_service(&mut dos, b"\\", 0x3b00, 0));
    success(&path_service(&mut dos, b"..", 0x3b00, 0));
    assert_eq!(current_directory(&mut dos, 0), b"");
}

async fn check_drive_services() {
    let mut dos = common::dos().await;
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    // Drive selection/query preserve CF. AH=47h reports absent drives through CF/AX.
    error(&service(&mut dos, 0x4700, 0, 0, 4), 15);
    let cpu = service(&mut dos, 0x1900, 0, 0, 0);
    assert_eq!(cpu.registers.eax as u8, 2);
    assert_eq!(cpu.flags.eflags() & 1, 1);
    for drive in [2, 3, 25] {
        let cpu = service(&mut dos, 0x0e00, 0, 0, drive);
        assert_eq!(cpu.registers.eax as u8, 5);
        assert_eq!(cpu.flags.eflags() & 1, 1);
        assert_eq!(service(&mut dos, 0x1900, 0, 0, 0).registers.eax as u8, 2);
    }
    let longest = "A".repeat(63);
    dos.mount_file(&format!("{longest}/X"), vec![], FileAccess::ReadOnly)
        .unwrap();
    success(&path_service(&mut dos, longest.as_bytes(), 0x3b00, 0));
    assert_eq!(current_directory(&mut dos, 0), longest.as_bytes());
    dos.mount_file(&format!("{longest}A/X"), vec![], FileAccess::ReadOnly)
        .unwrap();
    error(
        &path_service(&mut dos, format!("\\{longest}A").as_bytes(), 0x3b00, 0),
        3,
    );
    assert_eq!(current_directory(&mut dos, 0), longest.as_bytes());
}

async fn check_directory_changes() {
    let mut dos = common::dos().await;
    dos.mount_file("FILE.DAT", b"keep".to_vec(), FileAccess::ReadWrite)
        .unwrap();
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    error(&path_service(&mut dos, b"MISSING/CHILD", 0x3900, 0), 3);
    error(&path_service(&mut dos, b"MISSING", 0x4300, 0), 2);
    success(&path_service(&mut dos, b"save", 0x3900, 0));
    for (path, code) in [
        (b"SAVE".as_slice(), 5),
        (b"C:\\", 5),
        (b"FILE.DAT", 5),
        (b"FILE.DAT/CHILD", 3),
        (b"D:\\SAVE", 3),
        (b"*", 3),
    ] {
        error(&path_service(&mut dos, path, 0x3900, 0), code);
    }
    for (path, code) in [
        (b"C:\\".as_slice(), 5),
        (b"FILE.DAT", 5),
        (b"FILE.DAT/CHILD", 3),
        (b"MISSING", 3),
        (b"D:\\SAVE", 3),
        (b"*", 3),
    ] {
        error(&path_service(&mut dos, path, 0x3a00, 0), code);
    }
    assert_eq!(dos.read_file("FILE.DAT").unwrap(), b"keep");

    success(&path_service(&mut dos, b"C:SAVE", 0x3b00, 0));
    dos.cpu().context().cpu().set_eflags(0x0a57);
    let cpu = path_service(&mut dos, b"SLOT", 0x3900, 0);
    success(&cpu);
    assert_eq!(cpu.flags.eflags(), 0x0a56);
    let cpu = path_service(&mut dos, b"SLOT", 0x4300, 0);
    success(&cpu);
    assert_eq!(cpu.registers.ecx as u16, 0x10);
    let handle = success(&path_service(&mut dos, b"SLOT/GAME.SAV", 0x3c00, 0));
    success(&service(&mut dos, 0x3e00, handle, 0, 0));
    error(&path_service(&mut dos, b"SLOT", 0x3a00, 0), 5);
    assert_eq!(dos.read_file("SAVE/SLOT/GAME.SAV"), Some(vec![]));
    success(&path_service(&mut dos, b"SLOT/GAME.SAV", 0x4100, 0));
    success(&path_service(&mut dos, b"SLOT", 0x3a00, 0));
    error(&path_service(&mut dos, b"SLOT", 0x4300, 0), 2);
    error(&path_service(&mut dos, b".", 0x3a00, 0), 16);
    error(&path_service(&mut dos, b"C:\\SAVE", 0x3a00, 0), 16);
    assert_eq!(current_directory(&mut dos, 0), b"SAVE");

    // Removing and recreating a directory does not retain its old attributes.
    success(&path_service(&mut dos, b"TEMP", 0x3900, 0));
    success(&path_service(&mut dos, b"TEMP", 0x4301, 0x02));
    success(&path_service(&mut dos, b"TEMP", 0x3a00, 0));
    success(&path_service(&mut dos, b"TEMP", 0x3900, 0));
    dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
    assert_eq!(current_directory(&mut dos, 0), b"SAVE");
    let cpu = path_service(&mut dos, b"TEMP", 0x4300, 0);
    success(&cpu);
    assert_eq!(cpu.registers.ecx as u16, 0x10);
    success(&path_service(&mut dos, b"\\", 0x3b00, 0));
    error(&path_service(&mut dos, b"SAVE", 0x3a00, 0), 5);
    success(&path_service(&mut dos, b"SAVE/TEMP", 0x3a00, 0));
    success(&path_service(&mut dos, b"SAVE", 0x3a00, 0));
    error(&path_service(&mut dos, b"SAVE", 0x3b00, 0), 3);
}

#[test]
fn guest_paths_follow_the_current_directory() {
    pollster::block_on(check_directory_paths());
}

#[test]
fn drive_queries_and_directory_buffer() {
    pollster::block_on(check_drive_services());
}

#[test]
fn programs_create_and_remove_empty_directories() {
    pollster::block_on(check_directory_changes());
}

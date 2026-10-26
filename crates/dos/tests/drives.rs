mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess, MountError};
use relocade_vfs::MemoryFileSystem;
use std::io::ErrorKind;

fn read_guest_file(dos: &mut Dos, path: &[u8]) -> Vec<u8> {
    let handle = success(&path_service(dos, path, 0x3d00, 0));
    let length = success(&service(dos, 0x3f00, handle, 32, 0x500));
    let mut bytes = vec![0; usize::from(length)];
    dos.cpu()
        .context()
        .memory
        .read(0x10500, &mut bytes)
        .unwrap();
    success(&service(dos, 0x3e00, handle, 0, 0));
    bytes
}

fn dta(dos: &mut Dos) -> [u8; 43] {
    let mut bytes = [0; 43];
    dos.cpu()
        .context()
        .memory
        .read(0x10080, &mut bytes)
        .unwrap();
    bytes
}

fn found_name(data: &[u8; 43]) -> &[u8] {
    let bytes = &data[30..];
    &bytes[..bytes.iter().position(|byte| *byte == 0).unwrap()]
}

#[test]
fn mounted_drives_have_independent_directories_and_file_operations() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut disk = MemoryFileSystem::new();
        disk.create_dir("DATA").unwrap();
        disk.replace_file("DATA/FILE.DAT", b"D directory".to_vec())
            .unwrap();
        disk.replace_file("FILE.DAT", b"D root".to_vec()).unwrap();
        dos.mount('d', disk).unwrap();
        dos.mount_file(
            "DATA/FILE.DAT",
            b"C directory".to_vec(),
            FileAccess::ReadWrite,
        )
        .unwrap();
        dos.mount_file("FILE.DAT", b"C root".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        assert_eq!(
            dos.mount('1', MemoryFileSystem::new()).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            dos.mount_file("E:/FILE", vec![], FileAccess::ReadWrite),
            Err(MountError::InvalidPath)
        );
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();

        success(&path_service(&mut dos, b"DATA", 0x3b00, 0));
        success(&path_service(&mut dos, b"d:\\data", 0x3b00, 0));
        // CHDIR D: changes that drive's directory without selecting the drive.
        assert_eq!(service(&mut dos, 0x1900, 0, 0, 0).registers.eax as u8, 2);
        for (path, expected) in [
            (b"FILE.DAT".as_slice(), b"C directory".as_slice()),
            (b"D:FILE.DAT", b"D directory"),
            (b"d:\\file.dat", b"D root"),
        ] {
            assert_eq!(read_guest_file(&mut dos, path), expected);
        }
        dos.cpu().context().cpu.set_eflags(0x0a57);
        let cpu = service(&mut dos, 0x0e00, 0, 0, 3);
        assert_eq!(cpu.registers.eax as u8, 5);
        assert_eq!(cpu.flags.eflags(), 0x0a57);
        assert_eq!(service(&mut dos, 0x1900, 0, 0, 0).registers.eax as u8, 3);
        for (path, expected) in [
            (b"FILE.DAT".as_slice(), b"D directory".as_slice()),
            (b"\\FILE.DAT", b"D root"),
            (b"C:FILE.DAT", b"C directory"),
        ] {
            assert_eq!(read_guest_file(&mut dos, path), expected);
        }
        for number in [4, 26, 255] {
            service(&mut dos, 0x0e00, 0, 0, number);
            assert_eq!(service(&mut dos, 0x1900, 0, 0, 0).registers.eax as u8, 3);
        }
        dos.cpu().context().cpu.set_si(0x500);
        success(&service(&mut dos, 0x4700, 0, 0, 4));
        let mut directory = [0; 5];
        dos.cpu()
            .context()
            .memory
            .read(0x10500, &mut directory)
            .unwrap();
        assert_eq!(&directory, b"DATA\0");
        error(&service(&mut dos, 0x4700, 0, 0, 5), 15);

        // Host paths retain their C: root default, independently of guest state.
        dos.mount_file("HOST.DAT", b"host".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        assert_eq!(dos.read_file("HOST.DAT").unwrap(), b"host");
        assert_eq!(dos.read_file("D:/HOST.DAT"), None);
        let handle = success(&path_service(&mut dos, b"NEW.DAT", 0x5b00, 0x02));
        assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)) & 0x1f, 3);
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        assert_eq!(dos.read_file("D:/DATA/NEW.DAT"), Some(vec![]));
        assert_eq!(dos.read_file("DATA/NEW.DAT"), None);
        success(&path_service(&mut dos, b"FILE.DAT", 0x4301, 0x06));
        let cpu = path_service(&mut dos, b"C:FILE.DAT", 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0x20);

        // Rename uses the current drive for an omitted prefix and cannot cross drives.
        dos.cpu().context().cpu.set_di(0x600);
        dos.cpu()
            .context()
            .memory
            .write(0x10600, b"C:NEW.DAT\0")
            .unwrap();
        error(&path_service(&mut dos, b"NEW.DAT", 0x5600, 0), 17);
        dos.cpu()
            .context()
            .memory
            .write(0x10600, b"D:MOVED.DAT\0")
            .unwrap();
        success(&path_service(&mut dos, b"NEW.DAT", 0x5600, 0));
        success(&path_service(&mut dos, b"MOVED.DAT", 0x4100, 0));
        assert_eq!(dos.read_file("D:/DATA/MOVED.DAT"), None);
        success(&path_service(&mut dos, b"SUB", 0x3900, 0));
        success(&path_service(&mut dos, b"SUB", 0x3a00, 0));

        // An empty temporary-file path uses the selected drive's root.
        let handle = success(&path_service(&mut dos, b"", 0x5a00, 0));
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        let mut generated = [0; 10];
        dos.cpu()
            .context()
            .memory
            .read(0x10300, &mut generated)
            .unwrap();
        assert_eq!(generated[0], b'\\');
        assert_eq!(generated[9], 0);
        let name = std::str::from_utf8(&generated[1..9]).unwrap();
        assert_eq!(dos.read_file(&format!("D:/{name}")), Some(vec![]));
        assert_eq!(dos.read_file(name), None);
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(service(&mut dos, 0x1900, 0, 0, 0).registers.eax as u8, 3);
        assert_eq!(read_guest_file(&mut dos, b"FILE.DAT"), b"D directory");
        dos.mount('Z', MemoryFileSystem::new()).unwrap();
        assert_eq!(service(&mut dos, 0x0e00, 0, 0, 25).registers.eax as u8, 26);
    });
}

#[test]
fn remounting_preserves_handles_and_invalidates_only_that_drives_searches() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount('D', MemoryFileSystem::new()).unwrap();
        for drive in ['C', 'D'] {
            for name in ["A.DAT", "B.DAT"] {
                dos.mount_file(
                    &format!("{drive}:/DATA/{name}"),
                    b"old".to_vec(),
                    FileAccess::ReadWrite,
                )
                .unwrap();
            }
        }
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"D:\\DATA", 0x3b00, 0));
        let handle = success(&path_service(&mut dos, b"D:A.DAT", 0x3d42, 0));
        assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)) & 0x1f, 3);
        success(&service(&mut dos, 0x4000, handle, 0, 0));
        success(&path_service(&mut dos, b"D:*.DAT", 0x4e00, 0));
        let old_search = dta(&mut dos);
        assert_eq!(found_name(&old_search), b"A.DAT");
        success(&path_service(&mut dos, b"C:DATA/*.DAT", 0x4e00, 0));
        let c_search = dta(&mut dos);
        assert_eq!(found_name(&c_search), b"A.DAT");
        service(&mut dos, 0x0e00, 0, 0, 3);
        // Restoring D's search after a C: search and drive change keeps D's identity.
        dos.cpu()
            .context()
            .memory
            .write(0x10080, &old_search)
            .unwrap();
        success(&service(&mut dos, 0x4f00, 0, 0, 0));
        assert_eq!(found_name(&dta(&mut dos)), b"B.DAT");
        success(&path_service(&mut dos, b"D:A.DAT", 0x4301, 0x06));

        let mut disk = MemoryFileSystem::new();
        disk.create_dir("DATA").unwrap();
        disk.replace_file("DATA/A.DAT", b"replacement".to_vec())
            .unwrap();
        disk.replace_file("DATA/B.DAT", b"new".to_vec()).unwrap();
        disk.replace_file("ROOT.DAT", b"root".to_vec()).unwrap();
        dos.mount('d', disk).unwrap();
        assert_eq!(read_guest_file(&mut dos, b"ROOT.DAT"), b"root");
        let cpu = path_service(&mut dos, b"DATA/A.DAT", 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0x20);
        success(&path_service(&mut dos, b"DATA/A.DAT", 0x4301, 0));
        // The old handle still reads the old, truncated file rather than its replacement.
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 1, 0x500)), 0);
        // Closing the old written handle must not publish metadata on the new mount.
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        let cpu = path_service(&mut dos, b"DATA/A.DAT", 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0);
        assert_eq!(dos.read_file("D:/DATA/A.DAT").unwrap(), b"replacement");

        // A new search must not make an old DTA valid again after replacement.
        success(&path_service(&mut dos, b"DATA/*.DAT", 0x4e00, 0));
        dos.cpu()
            .context()
            .memory
            .write(0x10080, &old_search)
            .unwrap();
        error(&service(&mut dos, 0x4f00, 0, 0, 0), 18);
        dos.cpu()
            .context()
            .memory
            .write(0x10080, &c_search)
            .unwrap();
        success(&service(&mut dos, 0x4f00, 0, 0, 0));
        assert_eq!(found_name(&dta(&mut dos)), b"B.DAT");
    });
}

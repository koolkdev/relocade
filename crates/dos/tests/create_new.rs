mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;

#[test]
fn create_new_preserves_existing_files_and_returns_a_writable_handle() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA/KEEP.BIN", b"keep".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"DATA", 0x3b00, 0));
        success(&path_service(&mut dos, b"KEEP.BIN", 0x4301, 0x07));

        error(&path_service(&mut dos, b"c:keep.bin", 0x5b00, 0), 80);
        assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"keep");
        assert_eq!(
            path_service(&mut dos, b"KEEP.BIN", 0x4300, 0).registers.ecx,
            0x07
        );

        dos.runtime().machine().cpu().set_eflags(0x0a57);
        let cpu = path_service(&mut dos, b"NEW.BIN", 0x5b00, 0x03);
        let handle = success(&cpu);
        assert_eq!(handle, 5); // First slot after the five standard handles.
        assert_eq!(cpu.flags.eflags(), 0x0a56);
        assert_eq!(
            path_service(&mut dos, b"NEW.BIN", 0x4300, 0).registers.ecx,
            0x23
        );
        dos.runtime()
            .machine()
            .memory()
            .write(0x10400, b"new")
            .unwrap();
        assert_eq!(success(&service(&mut dos, 0x4000, handle, 3, 0x400)), 3);
        error(&path_service(&mut dos, b"NEW.BIN", 0x5b00, 0), 80);
        assert_eq!(dos.read_file("DATA/NEW.BIN").unwrap(), b"new");
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        error(&path_service(&mut dos, b"NEW.BIN", 0x3d02, 0), 5);

        success(&path_service(&mut dos, b"KEEP.BIN", 0x4301, 0));
        let handle = success(&path_service(&mut dos, b"KEEP.BIN", 0x3c00, 0));
        assert_eq!(dos.read_file("DATA/KEEP.BIN").unwrap(), b"");
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
    });
}

#[test]
fn create_new_validates_paths_attributes_and_capacity_before_publishing_a_file() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DIR/KEEP.BIN", b"keep".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for (path, attributes, code) in [
            (b"MISSING/NEW.BIN".as_slice(), 0, 3),
            (b"DIR", 0, 5),
            (b"NEW.BIN", 0x10, 5),
            (b"NEW.BIN", 0x08, 5),
            (b"NEW?.BIN", 0, 3),
        ] {
            error(&path_service(&mut dos, path, 0x5b00, attributes), code);
        }
        assert_eq!(dos.read_file("NEW.BIN"), None);

        // The process has twenty slots, five initially occupied by standard handles.
        for handle in 5..20 {
            let path = format!("FILE{handle}.BIN");
            assert_eq!(
                success(&path_service(&mut dos, path.as_bytes(), 0x5b00, 0)),
                handle
            );
        }
        error(&path_service(&mut dos, b"FULL.BIN", 0x5b00, 0), 4);
        assert_eq!(dos.read_file("FULL.BIN"), None);
        assert_eq!(dos.read_file("DIR/KEEP.BIN").unwrap(), b"keep");
        success(&service(&mut dos, 0x3e00, 5, 0, 0));
        assert_eq!(success(&path_service(&mut dos, b"FULL.BIN", 0x5b00, 0)), 5);
        assert_eq!(dos.read_file("FULL.BIN").unwrap(), b"");
    });
}

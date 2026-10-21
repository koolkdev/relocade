use crate::common::{self, error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;

#[test]
fn device_modes_are_shared_by_duplicates_and_reset_on_reopen_or_reload() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA.TXT", b"data".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let duplicate = success(&service(&mut dos, 0x4500, 0, 0, 0));
        // DOS always preserves the device bit, even when the caller omits it.
        let cpu = service(&mut dos, 0x4401, duplicate, 0x1234, 0x63);
        assert_eq!(success(&cpu), 0x44c3); // AL reports the previous device byte.
        assert_eq!(cpu.registers.edx as u16, 0x63);
        assert_eq!(cpu.registers.ecx as u16, 0x1234);
        for handle in [0, duplicate] {
            assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)), 0x80e3);
        }
        let reopened = success(&path_service(&mut dos, b"CON", 0x3d00, 0));
        assert_eq!(success(&service(&mut dos, 0x4400, reopened, 0, 0)), 0x80c3);
        // Changing mode does not change a read-only handle's access rights.
        success(&service(&mut dos, 0x4401, reopened, 0, 0xe3));
        error(&service(&mut dos, 0x4000, reopened, 0, 0), 5);
        success(&service(&mut dos, 0x4401, duplicate, 0, 0xc3));
        assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x80c3);

        error(&service(&mut dos, 0x4401, 0, 0, 0x80e3), 13);
        assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x80c3);
        let file = success(&path_service(&mut dos, b"DATA.TXT", 0x3d00, 0));
        error(&service(&mut dos, 0x4401, file, 0, 0xe3), 1);
        error(&service(&mut dos, 0x4401, 99, 0, 0xe3), 6);
        success(&service(&mut dos, 0x3e00, reopened, 0, 0));
        error(&service(&mut dos, 0x4401, reopened, 0, 0xe3), 6);
        success(&service(&mut dos, 0x4401, 0, 0, 0xe3));
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x80c3);
        error(&service(&mut dos, 0x4400, duplicate, 0, 0), 6);
    });
}

#[test]
fn raw_handle_output_preserves_control_bytes_and_cooked_output_still_expands_tabs() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.cpu()
            .context()
            .memory()
            .write(0x10400, b"A\t\x1aZ")
            .unwrap();
        success(&service(&mut dos, 0x4401, 1, 0, 0xe3));
        assert_eq!(success(&service(&mut dos, 0x4000, 1, 4, 0x400)), 4);
        assert_eq!(common::output(dos.stdout()), b"A\t\x1aZ");
        // Raw writes do not advance DOS's cooked-output column.
        success(&service(&mut dos, 0x4401, 1, 0, 0xc3));
        assert_eq!(success(&service(&mut dos, 0x4000, 1, 4, 0x400)), 2);
        assert_eq!(common::output(dos.stdout()), b"A       ");

        success(&service(&mut dos, 0x4401, 2, 0, 0xe3));
        assert_eq!(success(&service(&mut dos, 0x4000, 2, 4, 0x400)), 4);
        assert_eq!(common::output(dos.stderr()), b"A\t\x1aZ");
        assert!(common::output(dos.stdout()).is_empty());

        // Character services retain their own semantics even on a raw handle.
        success(&service(&mut dos, 0x4401, 1, 0, 0xe3));
        service(&mut dos, 0x0200, 0, 0, u16::from(b'\t'));
        assert_eq!(common::output(dos.stdout()), b"        ");
    });
}

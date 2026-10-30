use crate::common::{self, error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::FileAccess;
use std::io::Write;

#[test]
fn device_names_open_and_create_without_creating_vfs_files() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DIR/CON.TXT", b"disk".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for (path, info) in [
            (b"con".as_slice(), 0x80c3),
            (b"CON:", 0x80c3),
            (b"C:CON", 0x80c3),
            (b"C:\\DIR\\con.txt", 0x80c3),
            (b"NuL", 0x80c4),
            (b"NUL:", 0x80c4),
            (b"C:/DIR/NUL.TXT", 0x80c4),
        ] {
            // Create-new also opens devices, regardless of an existing filesystem name.
            for function in [0x3d02, 0x3c00, 0x5b00] {
                let handle = success(&path_service(&mut dos, path, function, 0));
                assert_eq!(handle, 5);
                assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)), info);
                success(&service(&mut dos, 0x3e00, handle, 0, 0));
            }
        }
        assert_eq!(dos.read_file("DIR/CON.TXT").unwrap(), b"disk");
        assert!(dos.read_file("CON").is_none());
        assert!(dos.read_file("DIR/NUL.TXT").is_none());
        for path in [b"MISSING\\NUL".as_slice(), b"Z:\\CON"] {
            error(&path_service(&mut dos, path, 0x3d00, 0), 3);
        }
        for path in [b"CONSOLE".as_slice(), b"NULFILE", b"X.CON"] {
            error(&path_service(&mut dos, path, 0x3d00, 0), 2);
        }
        error(&path_service(&mut dos, b"NUL", 0x3d03, 0), 12);
        for handle in 5..20 {
            assert_eq!(success(&path_service(&mut dos, b"NUL", 0x3d02, 0)), handle);
        }
        error(&path_service(&mut dos, b"CON", 0x3d02, 0), 4);
    });
}

#[test]
fn nul_reads_eof_discards_writes_and_can_redirect_standard_handles() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let handle = success(&path_service(&mut dos, b"NUL", 0x3d02, 0));
        dos.cpu()
            .with(|call| call.memory.write(0x10400, b"A\t\x1aZ").unwrap());
        assert_eq!(success(&service(&mut dos, 0x3f00, handle, 4, 0x400)), 0);
        let mut untouched = [0; 4];
        dos.cpu()
            .with(|call| call.memory.read(0x10400, &mut untouched).unwrap());
        assert_eq!(&untouched, b"A\t\x1aZ");
        assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)), 0x8084);
        assert_eq!(success(&service(&mut dos, 0x4000, handle, 4, 0x400)), 4);
        assert_eq!(success(&service(&mut dos, 0x4400, handle, 0, 0)), 0x80c4);
        for origin in 0..=2 {
            let cpu = service(&mut dos, 0x4200 | origin, handle, 0xffff, 0xffff);
            assert_eq!(success(&cpu), 0);
            assert_eq!(cpu.registers.edx as u16, 0);
        }
        error(&service(&mut dos, 0x4203, handle, 0, 0), 1);
        for function in [0x4406, 0x4407] {
            assert_eq!(
                success(&service(&mut dos, function, handle, 0, 0)) as u8,
                0xff
            );
        }
        success(&service(&mut dos, 0x4600, handle, 0, 0));
        success(&service(&mut dos, 0x4600, handle, 1, 0));
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        dos.stdin().write_all(b"K").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 0, 4, 0x400)), 0);
        assert_eq!(success(&service(&mut dos, 0x4000, 1, 4, 0x400)), 4);
        service(&mut dos, 0x0200, 0, 0, u16::from(b'X'));
        assert_eq!(service(&mut dos, 0x0b00, 0, 0, 0).registers.eax as u8, 0xff);
        for function in [0x0100, 0x0600, 0x0700, 0x0800] {
            let cpu = service(&mut dos, function, 0, 0, 0xff);
            assert_eq!(cpu.registers.eax as u8, 0);
            if function == 0x0600 {
                assert_eq!(cpu.flags.eflags() & 0x40, 0); // NUL is ready.
            }
            assert_eq!(success(&service(&mut dos, 0x4400, 0, 0, 0)), 0x80c4);
        }
        assert!(common::output(dos.stdout()).is_empty());
        assert!(dos.read_file("NUL").is_none());

        // Reopening CON reaches the keyboard even while stdin is redirected.
        let console = success(&path_service(&mut dos, b"CON", 0x3d00, 0));
        success(&service(&mut dos, 0x4600, console, 0, 0));
        assert_eq!(service(&mut dos, 0x0700, 0, 0, 0).registers.eax as u8, b'K');
    });
}

#[test]
fn console_opens_keep_eof_and_access_separate_while_duplicates_share_them() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let console = success(&path_service(&mut dos, b"CON", 0x3d02, 0));
        let duplicate = success(&service(&mut dos, 0x4500, console, 0, 0));
        dos.stdin().write_all(b"\x1a\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, console, 10, 0x400)), 0);
        assert_eq!(success(&service(&mut dos, 0x4400, duplicate, 0, 0)), 0x8083);
        let reopened = success(&path_service(&mut dos, b"CON", 0x3d02, 0));
        assert_eq!(success(&service(&mut dos, 0x4400, reopened, 0, 0)), 0x80c3);
        assert_eq!(success(&service(&mut dos, 0x4400, console, 0, 0)), 0x8083);
        success(&service(&mut dos, 0x3e00, console, 0, 0));
        assert_eq!(success(&service(&mut dos, 0x4000, duplicate, 0, 0x400)), 0);
        dos.stdin().write_all(b"ok\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, duplicate, 10, 0x400)), 4);
        let mut bytes = [0; 4];
        dos.cpu()
            .with(|call| call.memory.read(0x10400, &mut bytes).unwrap());
        assert_eq!(&bytes, b"ok\r\n");
        assert_eq!(common::output(dos.stdout()), b"^Z\r\nok\r\n");
        dos.stdin().write_all(b"E\r").unwrap();
        assert_eq!(success(&service(&mut dos, 0x3f00, 2, 3, 0x400)), 3);
        assert_eq!(common::output(dos.stderr()), b"E\r\n");

        for name in [b"CON", b"NUL"] {
            let reader = success(&path_service(&mut dos, name, 0x3d00, 0));
            let writer = success(&path_service(&mut dos, name, 0x3d01, 0));
            error(&service(&mut dos, 0x4000, reader, 1, 0x400), 5);
            error(&service(&mut dos, 0x3f00, writer, 1, 0x400), 5);
            let duplicate = success(&service(&mut dos, 0x4500, writer, 0, 0));
            error(&service(&mut dos, 0x3f00, duplicate, 1, 0x400), 5);
        }
        assert!(common::output(dos.stdout()).is_empty());
    });
}

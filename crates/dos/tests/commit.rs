mod common;

use common::{error, path_service, service, success, Backend, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};

fn attributes(dos: &mut Dos<Backend>, path: &[u8]) -> u16 {
    let cpu = path_service(dos, path, 0x4300, 0);
    success(&cpu);
    cpu.registers.ecx as u16
}

#[test]
fn commit_updates_metadata_without_closing_repositioning_or_releasing_sharing() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA.BIN", b"abcd".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0x06));
        let writer = success(&path_service(&mut dos, b"DATA.BIN", 0x3d22, 0));
        let duplicate = success(&service(&mut dos, 0x4500, writer, 0, 0));
        let reader = success(&path_service(&mut dos, b"DATA.BIN", 0x3d40, 0));
        dos.cpu().context().memory().write(0x10400, b"X").unwrap();
        success(&service(&mut dos, 0x4000, writer, 1, 0x400));

        // A separate read-only open has no pending metadata from the writer.
        success(&service(&mut dos, 0x6800, reader, 0, 0));
        assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x06);
        dos.cpu().context().cpu().set_eflags(0x0a57);
        // AH=6Ah aliases commit and returns AH=68h while preserving AL.
        let cpu = service(&mut dos, 0x6a37, duplicate, 0x1234, 0x5678);
        assert_eq!(success(&cpu), 0x6837);
        assert_eq!(cpu.flags.eflags(), 0x0a56);
        assert_eq!(cpu.registers.ebx as u16, duplicate);
        assert_eq!(cpu.registers.ecx as u16, 0x1234);
        assert_eq!(cpu.registers.edx as u16, 0x5678);
        assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x26);
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), b"Xbcd");

        assert_eq!(success(&service(&mut dos, 0x4201, duplicate, 0, 0)), 1);
        assert_eq!(success(&service(&mut dos, 0x3f00, writer, 1, 0x500)), 1);
        let mut byte = [0];
        dos.cpu()
            .context()
            .memory()
            .read(0x10500, &mut byte)
            .unwrap();
        assert_eq!(&byte, b"b");
        assert_eq!(success(&service(&mut dos, 0x4201, duplicate, 0, 0)), 2);
        error(&path_service(&mut dos, b"DATA.BIN", 0x3d41, 0), 5);
        success(&service(&mut dos, 0x3e00, writer, 0, 0));
        error(&service(&mut dos, 0x6800, writer, 0, 0), 6);
        success(&service(&mut dos, 0x6800, duplicate, 0, 0));
        error(&path_service(&mut dos, b"DATA.BIN", 0x3d41, 0), 5);

        // The surviving duplicate remains writable, and later commits publish metadata.
        success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0x06));
        success(&service(&mut dos, 0x4000, duplicate, 1, 0x400));
        success(&service(&mut dos, 0x6800, duplicate, 0, 0));
        assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x26);
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), b"XbXd");
        success(&service(&mut dos, 0x3e00, duplicate, 0, 0));
        success(&path_service(&mut dos, b"DATA.BIN", 0x3d41, 0));
    });
}

#[test]
fn commit_validates_handles_and_respects_device_and_replaced_file_identity() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("DATA.BIN", b"old".to_vec(), FileAccess::ReadWrite)
            .unwrap();
        dos.load_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for handle in [5, 20, u16::MAX] {
            error(&service(&mut dos, 0x6800, handle, 0, 0), 6);
        }
        error(&service(&mut dos, 0x6a37, 5, 0, 0), 6);
        let old = success(&path_service(&mut dos, b"DATA.BIN", 0x3d42, 0));
        dos.cpu().context().memory().write(0x10400, b"X").unwrap();
        success(&service(&mut dos, 0x4000, old, 1, 0x400));
        dos.mount_file("DATA.BIN", b"replacement".to_vec(), FileAccess::ReadOnly)
            .unwrap();
        success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0x07));
        success(&service(&mut dos, 0x6800, old, 0, 0));
        assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x07);
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), b"replacement");
        assert_eq!(success(&service(&mut dos, 0x3f00, old, 2, 0x500)), 2);
        let mut bytes = [0; 2];
        dos.cpu()
            .context()
            .memory()
            .read(0x10500, &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"ld");

        let reader = success(&path_service(&mut dos, b"DATA.BIN", 0x3d40, 0));
        // Committing needs no spare handle, including for read-only files and devices.
        for handle in 7..20 {
            assert_eq!(success(&service(&mut dos, 0x4500, 1, 0, 0)), handle);
        }
        success(&service(&mut dos, 0x6800, reader, 0, 0));
        assert_eq!(attributes(&mut dos, b"DATA.BIN"), 0x07);
        success(&service(&mut dos, 0x4000, 1, 1, 0x400));
        for handle in 0..5 {
            success(&service(&mut dos, 0x6800, handle, 0, 0));
        }
        assert_eq!(success(&service(&mut dos, 0x6a37, 1, 0, 0)), 0x6837);
        success(&service(&mut dos, 0x4000, 1, 1, 0x400));
        assert_eq!(common::output(dos.stdout()), b"XX");
        success(&service(&mut dos, 0x3e00, 19, 0, 0));
        error(&service(&mut dos, 0x6a37, 19, 0, 0), 6);
    });
}

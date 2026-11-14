mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{FileAccess, MountError};
use relocade_vfs::MemoryFileSystem;
use std::io::ErrorKind;

#[test]
fn dos_mounts_validate_geometry_before_replacing_a_drive() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("KEEP.BIN", vec![1], FileAccess::ReadOnly)
            .unwrap();
        for (capacity, unit) in [(512, 256), (1536, 1536), (65536, 65536), (65536 * 512, 512)] {
            let disk = MemoryFileSystem::with_capacity(capacity, unit).unwrap();
            assert_eq!(
                dos.mount('C', disk).unwrap_err().kind(),
                ErrorKind::InvalidInput
            );
            assert_eq!(dos.read_file("KEEP.BIN").unwrap(), [1]);
        }
        for unit in [512, 1024, 32768] {
            let disk = MemoryFileSystem::with_capacity(65535 * u64::from(unit), unit).unwrap();
            dos.mount('D', disk).unwrap();
        }
    });
}

#[test]
fn host_file_replacement_reports_full_volumes_and_keeps_previous_contents() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount('C', MemoryFileSystem::with_capacity(1024, 512).unwrap())
            .unwrap();
        dos.mount_file("DATA.BIN", vec![1; 1024], FileAccess::ReadOnly)
            .unwrap();
        assert_eq!(
            dos.mount_file("DATA.BIN", vec![2; 1025], FileAccess::ReadWrite),
            Err(MountError::DiskFull)
        );
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), vec![1; 1024]);
        dos.mount_file("DATA.BIN", vec![2; 1024], FileAccess::ReadWrite)
            .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let old = success(&path_service(&mut dos, b"DATA.BIN", 0x3d00, 0));
        assert_eq!(
            dos.mount_file("DATA.BIN", vec![3], FileAccess::ReadWrite),
            Err(MountError::DiskFull)
        );
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), vec![2; 1024]);
        success(&service(&mut dos, 0x3e00, old, 0, 0));
        dos.mount_file("DATA.BIN", vec![3], FileAccess::ReadWrite)
            .unwrap();
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), [3]);
    });
}

#[test]
fn full_disk_writes_return_actual_counts_and_preserve_unwritten_metadata() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount('C', MemoryFileSystem::with_capacity(1024, 512).unwrap())
            .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let writer = success(&path_service(&mut dos, b"DATA.BIN", 0x3c00, 0));
        dos.cpu()
            .memory()
            .view()
            .write(0x10400, &[0x5a; 1536])
            .unwrap();
        dos.cpu().cpu_state().set_eflags(0x0a57);
        let cpu = service(&mut dos, 0x4000, writer, 1536, 0x400);
        assert_eq!(success(&cpu), 1024);
        assert_eq!(cpu.flags.eflags(), 0x0a56);
        assert_eq!(cpu.registers.ebx as u16, writer);
        assert_eq!(cpu.registers.ecx as u16, 1536);
        assert_eq!(cpu.registers.edx as u16, 0x400);
        assert_eq!(success(&service(&mut dos, 0x4201, writer, 0, 0)), 1024);
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), vec![0x5a; 1024]);
        success(&service(&mut dos, 0x5701, writer, 0x645c, 0x285d));
        assert_eq!(success(&service(&mut dos, 0x4000, writer, 1, 0x400)), 0);
        assert_eq!(success(&service(&mut dos, 0x4201, writer, 0, 0)), 1024);
        // Failed DOS 5 zero-length extensions leave the size and chosen timestamp intact.
        for position in [1025u32, 0x8000_0000] {
            success(&service(
                &mut dos,
                0x4200,
                writer,
                (position >> 16) as u16,
                position as u16,
            ));
            assert_eq!(success(&service(&mut dos, 0x4000, writer, 0, 0)), 0);
            assert_eq!(success(&service(&mut dos, 0x4202, writer, 0, 0)), 1024);
        }
        success(&service(&mut dos, 0x6800, writer, 0, 0));
        let cpu = service(&mut dos, 0x5700, writer, 0, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0x645c);
        assert_eq!(cpu.registers.edx as u16, 0x285d);
        success(&service(&mut dos, 0x3e00, writer, 0, 0));
        success(&path_service(&mut dos, b"DATA.BIN", 0x4301, 0));
        let writer = success(&path_service(&mut dos, b"DATA.BIN", 0x3d02, 0));
        success(&service(&mut dos, 0x4202, writer, 0, 0));
        assert_eq!(success(&service(&mut dos, 0x4000, writer, 1, 0x400)), 0);
        success(&service(&mut dos, 0x3e00, writer, 0, 0));
        let cpu = path_service(&mut dos, b"DATA.BIN", 0x4300, 0);
        success(&cpu);
        assert_eq!(cpu.registers.ecx as u16, 0); // No bytes written, so archive stays clear.

        let writer = success(&path_service(&mut dos, b"DATA.BIN", 0x3d02, 0));
        success(&service(&mut dos, 0x4200, writer, 0, 256));
        success(&service(&mut dos, 0x4000, writer, 0, 0));
        assert_eq!(dos.read_file("DATA.BIN").unwrap().len(), 256);
        let other = success(&path_service(&mut dos, b"OTHER.BIN", 0x3c00, 0));
        assert_eq!(success(&service(&mut dos, 0x4000, other, 1024, 0x400)), 512);
        let reader = success(&path_service(&mut dos, b"DATA.BIN", 0x3d00, 0));
        error(&service(&mut dos, 0x4000, reader, 1, 0x400), 5);
    });
}

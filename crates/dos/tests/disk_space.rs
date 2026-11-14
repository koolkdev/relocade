mod common;

use common::{path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};
use relocade_vfs::MemoryFileSystem;
use std::io::ErrorKind;

fn free_clusters(dos: &mut Dos, drive: u8) -> u16 {
    let cpu = service(dos, 0x3600, 0, 0, u16::from(drive));
    assert_ne!(cpu.registers.eax as u16, 0xffff);
    assert_eq!(cpu.registers.ecx as u16, 512);
    cpu.registers.ebx as u16
}

#[test]
fn disk_space_returns_geometry_preserves_other_state_and_validates_drive_numbers() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut disk = MemoryFileSystem::new();
        disk.replace_file("DATA.BIN", vec![1; 2049]).unwrap();
        dos.mount('D', disk).unwrap();
        dos.set_drive_capacity('D', 8192).unwrap();
        dos.mount('A', MemoryFileSystem::new()).unwrap();
        dos.set_drive_capacity('A', 4096).unwrap();
        dos.mount('Z', MemoryFileSystem::new()).unwrap();
        dos.set_drive_capacity('Z', 1024).unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for (drive, sectors, free, total) in [
            (0u8, 32u16, 32768u16, 32768u16),
            (1, 1, 8, 8),
            (4, 1, 11, 16),
            (26, 1, 2, 2),
        ] {
            let state = dos.cpu().cpu_state();
            state.set_eax(0xabcd_36ff);
            state.set_ebx(0x1234_dead);
            state.set_ecx(0x2345_beef);
            state.set_edx(0x3456_ab00 | u32::from(drive));
            state.set_eflags(0x0a57);
            let mut expected = state.snapshot();
            expected.registers.eax = 0xabcd_0000 | u32::from(sectors);
            expected.registers.ebx = 0x1234_0000 | u32::from(free);
            expected.registers.ecx = 0x2345_0200;
            expected.registers.edx = 0x3456_0000 | u32::from(total);
            let cpu = service(&mut dos, 0x36ff, 0xdead, 0xbeef, 0xab00 | u16::from(drive));
            assert_eq!(cpu.registers, expected.registers);
            assert_eq!(cpu.segments, expected.segments);
            assert_eq!(cpu.flags.eflags(), 0x0a57);
        }
        for drive in [2u8, 5, 27, 255] {
            let state = dos.cpu().cpu_state();
            state.set_eax(0xabcd_3600);
            state.set_ebx(0x1234_dead);
            state.set_ecx(0x2345_beef);
            state.set_edx(0x3456_ab00 | u32::from(drive));
            state.set_eflags(0x0856);
            let mut expected = state.snapshot();
            expected.registers.eax = 0xabcd_ffff;
            let cpu = service(&mut dos, 0x3600, 0xdead, 0xbeef, 0xab00 | u16::from(drive));
            assert_eq!(cpu.registers, expected.registers);
            assert_eq!(cpu.segments, expected.segments);
            assert_eq!(cpu.flags.eflags(), 0x0856);
        }
        service(&mut dos, 0x0e00, 0, 0, 3); // Select D:; AH=0Eh uses zero-based numbering.
        assert_eq!(free_clusters(&mut dos, 0), 11);
        assert_eq!(free_clusters(&mut dos, 3), 32768);
        assert_eq!(service(&mut dos, 0x1900, 0, 0, 0).registers.eax as u8, 3);
    });
}

#[test]
fn capacity_selects_representable_geometry_and_rejects_invalid_settings() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for (capacity, sectors, total) in [
            (0u64, 1u16, 0u16),
            (511, 1, 0),
            (512, 1, 1),
            (513, 1, 1),
            (65535 * 512, 1, 65535),
            (65535 * 512 + 511, 1, 65535),
            (65536 * 512, 2, 32768),
            (512 * 1024 * 1024, 32, 32768),
            (65535 * 32768, 64, 65535),
        ] {
            dos.set_drive_capacity('c', capacity).unwrap();
            let cpu = service(&mut dos, 0x3600, 0, 0, 0);
            assert_eq!(cpu.registers.eax as u16, sectors);
            assert_eq!(cpu.registers.ebx as u16, total);
            assert_eq!(cpu.registers.ecx as u16, 512);
            assert_eq!(cpu.registers.edx as u16, total);
        }
        dos.set_drive_capacity('C', 1024).unwrap();
        for (letter, capacity, error) in [
            ('C', 65535 * 32768 + 1, ErrorKind::InvalidInput),
            ('C', u64::MAX, ErrorKind::InvalidInput),
            ('@', 512, ErrorKind::InvalidInput),
            ('B', 512, ErrorKind::NotFound),
        ] {
            assert_eq!(
                dos.set_drive_capacity(letter, capacity).unwrap_err().kind(),
                error
            );
            assert_eq!(free_clusters(&mut dos, 0), 2);
        }
        // Host imports also remain independent of the reported capacity.
        dos.mount_file("LARGE.BIN", vec![1; 1536], FileAccess::ReadWrite)
            .unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 0);
        assert_eq!(dos.read_file("LARGE.BIN").unwrap().len(), 1536);
    });
}

#[test]
fn reported_usage_tracks_guest_file_sizes_without_limiting_writes() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.set_drive_capacity('C', 2048).unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        dos.cpu()
            .memory()
            .view()
            .write(0x10400, &[0x5a; 1500])
            .unwrap();
        success(&path_service(&mut dos, b"DIR", 0x3900, 0));
        let file = success(&path_service(&mut dos, b"DATA.BIN", 0x3c00, 0));
        assert_eq!(free_clusters(&mut dos, 0), 4);
        success(&service(&mut dos, 0x4000, file, 1, 0x400));
        assert_eq!(free_clusters(&mut dos, 0), 3);
        let duplicate = success(&service(&mut dos, 0x4500, file, 0, 0));
        let other = success(&path_service(&mut dos, b"OTHER.BIN", 0x5b00, 0));
        success(&service(&mut dos, 0x4000, other, 1, 0x400));
        assert_eq!(free_clusters(&mut dos, 0), 3); // Two bytes total, rounded once.
        success(&service(&mut dos, 0x4000, duplicate, 511, 0x400));
        assert_eq!(free_clusters(&mut dos, 0), 2);
        success(&service(&mut dos, 0x4000, file, 1, 0x400));
        assert_eq!(free_clusters(&mut dos, 0), 2);
        assert_eq!(success(&service(&mut dos, 0x4000, file, 1500, 0x400)), 1500);
        assert_eq!(free_clusters(&mut dos, 0), 0);
        assert_eq!(success(&service(&mut dos, 0x4000, file, 1500, 0x400)), 1500);
        assert_eq!(dos.read_file("DATA.BIN").unwrap().len(), 3513);
        assert_eq!(free_clusters(&mut dos, 0), 0);
        success(&service(&mut dos, 0x4200, duplicate, 0, 4096));
        success(&service(&mut dos, 0x4000, duplicate, 0, 0));
        assert_eq!(dos.read_file("DATA.BIN").unwrap().len(), 4096);
        assert_eq!(free_clusters(&mut dos, 0), 0);
        success(&service(&mut dos, 0x4200, duplicate, 0, 512));
        success(&service(&mut dos, 0x4000, duplicate, 0, 0));
        assert_eq!(free_clusters(&mut dos, 0), 2);
        success(&service(&mut dos, 0x3e00, file, 0, 0));
        success(&service(&mut dos, 0x3e00, duplicate, 0, 0));
        assert_eq!(free_clusters(&mut dos, 0), 2);
        dos.cpu()
            .memory()
            .view()
            .write(0x10400, b"MOVED.BIN\0")
            .unwrap();
        dos.cpu().cpu_state().set_di(0x400);
        success(&path_service(&mut dos, b"DATA.BIN", 0x5600, 0));
        assert_eq!(free_clusters(&mut dos, 0), 2);
        success(&path_service(&mut dos, b"MOVED.BIN", 0x4100, 0));
        assert_eq!(free_clusters(&mut dos, 0), 3);
        success(&service(&mut dos, 0x3e00, other, 0, 0));
        success(&path_service(&mut dos, b"OTHER.BIN", 0x4100, 0));
        assert_eq!(free_clusters(&mut dos, 0), 4);
    });
}

#[test]
fn reporting_follows_current_files_and_remounting_resets_capacity() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut disk = MemoryFileSystem::new();
        let host_file = disk.replace_file("DATA.BIN", vec![1]).unwrap();
        dos.mount('C', disk).unwrap();
        dos.set_drive_capacity('C', 1536).unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 2);
        host_file.set_len(513).unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 1);
        let old = success(&path_service(&mut dos, b"DATA.BIN", 0x3d00, 0));
        dos.mount_file("DATA.BIN", vec![2], FileAccess::ReadWrite)
            .unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 2);
        host_file.set_len(2000).unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 2);
        drop(host_file);
        success(&service(&mut dos, 0x3e00, old, 0, 0));
        assert_eq!(free_clusters(&mut dos, 0), 2);
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 2);
        let old = success(&path_service(&mut dos, b"DATA.BIN", 0x3d02, 0));
        let mut replacement = MemoryFileSystem::new();
        replacement.replace_file("DATA.BIN", vec![3; 513]).unwrap();
        dos.mount('C', replacement).unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 32767);
        dos.set_drive_capacity('C', 2048).unwrap();
        assert_eq!(free_clusters(&mut dos, 0), 2);
        dos.cpu()
            .memory()
            .view()
            .write(0x10400, &[4; 1536])
            .unwrap();
        assert_eq!(success(&service(&mut dos, 0x4000, old, 1536, 0x400)), 1536);
        success(&service(&mut dos, 0x3e00, old, 0, 0));
        assert_eq!(free_clusters(&mut dos, 0), 2);
        assert_eq!(dos.read_file("DATA.BIN").unwrap(), vec![3; 513]);
    });
}

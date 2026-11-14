mod common;

use common::{error, path_service, service, success, SERVICE_PROGRAM};
use relocade_dos::{Dos, FileAccess};
use relocade_vfs::MemoryFileSystem;

fn timestamp(dos: &mut Dos, handle: u16) -> (u16, u16) {
    let cpu = service(dos, 0x5700, handle, 0, 0);
    success(&cpu);
    (cpu.registers.ecx as u16, cpu.registers.edx as u16)
}

fn dta_timestamp(dos: &mut Dos) -> (u16, u16) {
    let mut words = [0; 4];
    dos.cpu()
        .memory()
        .view()
        .read(0x10080 + 22, &mut words)
        .unwrap();
    (
        u16::from_le_bytes([words[0], words[1]]),
        u16::from_le_bytes([words[2], words[3]]),
    )
}

fn found_timestamp(dos: &mut Dos, path: &[u8]) -> (u16, u16) {
    success(&path_service(dos, path, 0x4e00, 0x10));
    dta_timestamp(dos)
}

fn set_clock_date(dos: &mut Dos, year: u16, month_day: u16) {
    // Keep date assertions away from midnight without relying on elapsed seconds.
    assert_eq!(service(dos, 0x2d00, 0, 0x0c00, 0).registers.eax as u8, 0);
    assert_eq!(
        service(dos, 0x2b00, 0, year, month_day).registers.eax as u8,
        0
    );
}

#[test]
fn timestamp_services_preserve_packed_words_registers_and_flags_and_report_errors() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.mount_file("READONLY.BIN", vec![1], FileAccess::ReadOnly)
            .unwrap();
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        for handle in [5, 20, u16::MAX] {
            for function in [0x5700, 0x5701] {
                let cpu = service(&mut dos, function, handle, 0x1234, 0x5678);
                error(&cpu, 6);
                assert_eq!(cpu.registers.ecx as u16, 0x1234);
                assert_eq!(cpu.registers.edx as u16, 0x5678);
            }
        }
        let handle = success(&path_service(&mut dos, b"READONLY.BIN", 0x3d00, 0));
        // AH=57h accepts raw directory words, including dates beyond AH=2Bh's range
        // and bit patterns that do not describe a valid calendar date or time.
        for (time, date) in [(0x645c, 0x285d), (0xbf7d, 0xff9f), (0xffff, 0), (0, 0xffff)] {
            let state = dos.cpu().cpu_state();
            state.set_eax(0xabcd_5701);
            state.set_ebx(0x1234_0000 | u32::from(handle));
            state.set_ecx(0x2345_0000 | u32::from(time));
            state.set_edx(0x3456_0000 | u32::from(date));
            state.set_eflags(0x0a57);
            let expected = state.snapshot();
            let cpu = service(&mut dos, 0x5701, handle, time, date);
            assert_eq!(cpu.registers, expected.registers);
            assert_eq!(cpu.segments, expected.segments);
            assert_eq!(cpu.flags.eflags(), 0x0a56);

            let state = dos.cpu().cpu_state();
            state.set_eax(0xabcd_5700);
            state.set_ecx(0x2345_dead);
            state.set_edx(0x3456_beef);
            state.set_eflags(0x0857);
            let mut expected = state.snapshot();
            expected.registers.ecx = 0x2345_0000 | u32::from(time);
            expected.registers.edx = 0x3456_0000 | u32::from(date);
            let cpu = service(&mut dos, 0x5700, handle, 0xdead, 0xbeef);
            assert_eq!(cpu.registers, expected.registers);
            assert_eq!(cpu.segments, expected.segments);
            assert_eq!(cpu.flags.eflags(), 0x0856);
        }
        for handle in [handle, u16::MAX] {
            for function in [0x5702, 0x57ff] {
                error(&service(&mut dos, function, handle, 0, 0), 1);
            }
        }
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        error(&service(&mut dos, 0x5700, handle, 0, 0), 6);
        assert_eq!(found_timestamp(&mut dos, b"READONLY.BIN"), (0, 0xffff));
    });
}

#[test]
fn duplicates_share_pending_timestamps_and_commit_publishes_them_to_searches() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        set_clock_date(&mut dos, 2000, 0x021d);
        let writer = success(&path_service(&mut dos, b"A.BIN", 0x3c00, 0));
        let initial = timestamp(&mut dos, writer);
        assert_eq!(initial.1, 0x285d);
        assert_eq!(found_timestamp(&mut dos, b"A.BIN"), initial);
        let duplicate = success(&service(&mut dos, 0x4500, writer, 0, 0));
        let reader = success(&path_service(&mut dos, b"A.BIN", 0x3d00, 0));
        dos.cpu().memory().view().write(0x10400, b"abc").unwrap();
        success(&service(&mut dos, 0x4000, writer, 3, 0x400));
        // A copy utility restores the source timestamp after writing the destination.
        success(&service(&mut dos, 0x5701, writer, 0x645c, 0x585d));
        assert_eq!(timestamp(&mut dos, duplicate), (0x645c, 0x585d));
        assert_eq!(timestamp(&mut dos, reader), initial);
        success(&service(&mut dos, 0x6800, reader, 0, 0));
        assert_eq!(found_timestamp(&mut dos, b"A.BIN"), initial);
        success(&service(&mut dos, 0x6a00, duplicate, 0, 0));
        assert_eq!(found_timestamp(&mut dos, b"A.BIN"), (0x645c, 0x585d));
        success(&service(&mut dos, 0x3e00, writer, 0, 0));
        assert_eq!(timestamp(&mut dos, duplicate), (0x645c, 0x585d));

        // Ordinary writes and zero-length resizing both restore automatic stamping.
        for count in [1, 0] {
            success(&service(&mut dos, 0x5701, duplicate, 0x645c, 0x585d));
            set_clock_date(&mut dos, 2050, 0x0102);
            success(&service(&mut dos, 0x4000, duplicate, count, 0x400));
            assert_eq!(timestamp(&mut dos, duplicate), (0x645c, 0x585d));
            // The timestamp is sampled when committed, not when the data was written.
            set_clock_date(&mut dos, 2051, 0x0103);
            success(&service(&mut dos, 0x6800, duplicate, 0, 0));
            let committed = timestamp(&mut dos, duplicate);
            assert_eq!(committed.1, 0x8e23);
            assert_eq!(found_timestamp(&mut dos, b"A.BIN"), committed);
        }
        // Closing an unmodified independent open cannot restore its old timestamp.
        success(&service(&mut dos, 0x3e00, reader, 0, 0));
        assert_eq!(found_timestamp(&mut dos, b"A.BIN").1, 0x8e23);
        success(&service(&mut dos, 0x5701, duplicate, 0x645c, 0x585d));
        success(&service(&mut dos, 0x3e00, duplicate, 0, 0));
        let second = success(&path_service(&mut dos, b"B.BIN", 0x5b00, 0));
        success(&service(&mut dos, 0x5701, second, 0xbf7d, 0xff9f));
        success(&service(&mut dos, 0x3e00, second, 0, 0));
        assert_eq!(found_timestamp(&mut dos, b"*.BIN"), (0x645c, 0x585d));
        success(&service(&mut dos, 0x4f00, 0, 0, 0));
        assert_eq!(dta_timestamp(&mut dos), (0xbf7d, 0xff9f));
    });
}

#[test]
fn timestamp_updates_follow_file_identity_rename_and_process_cleanup() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let old = success(&path_service(&mut dos, b"DATA.BIN", 0x3c00, 0));
        success(&service(&mut dos, 0x5701, old, 0x645c, 0x285d));
        for remount in [false, true] {
            set_clock_date(&mut dos, 2050, 0x0102);
            if remount {
                let mut disk = MemoryFileSystem::new();
                disk.replace_file("DATA.BIN", b"new drive".to_vec())
                    .unwrap();
                dos.mount('C', disk).unwrap();
            } else {
                dos.mount_file("DATA.BIN", b"new file".to_vec(), FileAccess::ReadWrite)
                    .unwrap();
            }
            let replacement = found_timestamp(&mut dos, b"DATA.BIN");
            assert_eq!(replacement.1, 0x8c22);
            set_clock_date(&mut dos, 2051, 0x0103);
            success(&service(&mut dos, 0x6800, old, 0, 0));
            assert_eq!(timestamp(&mut dos, old), (0x645c, 0x285d));
            assert_eq!(found_timestamp(&mut dos, b"DATA.BIN"), replacement);
        }
        success(&service(&mut dos, 0x3e00, old, 0, 0));
        let current = success(&path_service(&mut dos, b"DATA.BIN", 0x3d00, 0));
        success(&service(&mut dos, 0x5701, current, 0xbf7d, 0xff9f));
        // Forced duplication closes the destination and publishes its pending metadata.
        success(&service(&mut dos, 0x4600, 1, current, 0));
        assert_eq!(found_timestamp(&mut dos, b"DATA.BIN"), (0xbf7d, 0xff9f));
        dos.cpu()
            .memory()
            .view()
            .write(0x10400, b"MOVED.BIN\0")
            .unwrap();
        dos.cpu().cpu_state().set_di(0x400);
        success(&path_service(&mut dos, b"DATA.BIN", 0x5600, 0));
        assert_eq!(found_timestamp(&mut dos, b"MOVED.BIN"), (0xbf7d, 0xff9f));
        success(&path_service(&mut dos, b"MOVED.BIN", 0x4301, 0x01));
        assert_eq!(found_timestamp(&mut dos, b"MOVED.BIN"), (0xbf7d, 0xff9f));
        let current = success(&path_service(&mut dos, b"MOVED.BIN", 0x3d00, 0));
        success(&service(&mut dos, 0x5701, current, 0x645c, 0x585d));
        success(&service(&mut dos, 0x4500, current, 0, 0));
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let reopened = success(&path_service(&mut dos, b"MOVED.BIN", 0x3d00, 0));
        assert_eq!(timestamp(&mut dos, reopened), (0x645c, 0x585d));
        success(&service(&mut dos, 0x3e00, reopened, 0, 0));
        success(&path_service(&mut dos, b"MOVED.BIN", 0x4301, 0));
        success(&path_service(&mut dos, b"MOVED.BIN", 0x4100, 0));
        let recreated = success(&path_service(&mut dos, b"MOVED.BIN", 0x3c00, 0));
        assert_eq!(timestamp(&mut dos, recreated).1, 0x8e23);
    });
}

#[test]
fn directory_and_truncated_file_timestamps_use_the_dos_clock() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        set_clock_date(&mut dos, 2000, 0x021d);
        success(&path_service(&mut dos, b"DIR", 0x3900, 0));
        let created = found_timestamp(&mut dos, b"DIR");
        assert_eq!(created.1, 0x285d);
        set_clock_date(&mut dos, 2050, 0x0102);
        assert_eq!(found_timestamp(&mut dos, b"DIR/.").1, 0x285d);
        assert_eq!(found_timestamp(&mut dos, b"DIR"), created);
        dos.mount_file("DIR/DATA.BIN", vec![1, 2, 3], FileAccess::ReadWrite)
            .unwrap();
        let initial = found_timestamp(&mut dos, b"DIR/DATA.BIN");
        assert_eq!(initial.1, 0x8c22);
        let reader = success(&path_service(&mut dos, b"DIR/DATA.BIN", 0x3d00, 0));
        error(&service(&mut dos, 0x4000, reader, 0, 0), 5);
        set_clock_date(&mut dos, 2051, 0x0103);
        success(&service(&mut dos, 0x3e00, reader, 0, 0));
        assert_eq!(found_timestamp(&mut dos, b"DIR/DATA.BIN"), initial);
        let truncated = success(&path_service(&mut dos, b"DIR/DATA.BIN", 0x3c00, 0));
        assert_eq!(timestamp(&mut dos, truncated).1, 0x8e23);
        set_clock_date(&mut dos, 2052, 0x0104);
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        assert_eq!(found_timestamp(&mut dos, b"DIR/DATA.BIN").1, 0x9024);
    });
}

#[test]
fn character_device_timestamps_belong_to_the_open_record() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let handle = success(&path_service(&mut dos, b"NUL", 0x3d00, 0));
        let duplicate = success(&service(&mut dos, 0x4500, handle, 0, 0));
        let other = success(&path_service(&mut dos, b"NUL", 0x3d00, 0));
        let initial = timestamp(&mut dos, other);
        success(&service(&mut dos, 0x5701, handle, 0x645c, 0x585d));
        success(&service(&mut dos, 0x6800, handle, 0, 0));
        success(&service(&mut dos, 0x3e00, handle, 0, 0));
        assert_eq!(timestamp(&mut dos, duplicate), (0x645c, 0x585d));
        assert_eq!(timestamp(&mut dos, other), initial);
    });
}

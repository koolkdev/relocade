mod common;

use chrono::{Datelike, Local, NaiveDate, Timelike};
use common::{service, SERVICE_PROGRAM};

#[test]
fn date_reads_the_host_calendar_and_preserves_other_registers_and_bios_time() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let state = dos.cpu().cpu_state();
        state.set_eax(0xabcd_2aff);
        state.set_ebx(0x1234_5678);
        state.set_ecx(0x2345_0000);
        state.set_edx(0x3456_0000);
        state.set_eflags(0x0a57);
        let mut expected = state.snapshot();
        let memory = dos.cpu().memory();
        let bios_time = [0x12, 0x34, 0x56, 0x78, 7];
        memory.view().write(0x46c, &bios_time).unwrap();

        let before = Local::now().date_naive();
        let cpu = service(&mut dos, 0x2aff, 0x5678, 0, 0);
        let after = Local::now().date_naive();
        let date = NaiveDate::from_ymd_opt(
            i32::from(cpu.registers.ecx as u16),
            (cpu.registers.edx >> 8) & 0xff,
            cpu.registers.edx & 0xff,
        )
        .unwrap();
        assert!((before..=after).contains(&date));
        expected.registers.eax = 0xabcd_2a00 | date.weekday().num_days_from_sunday();
        expected.registers.ecx |= date.year() as u32;
        expected.registers.edx |= (date.month() << 8) | date.day();
        assert_eq!(cpu.registers, expected.registers);
        assert_eq!(cpu.segments, expected.segments);
        assert_eq!(cpu.flags.eflags(), 0x0a57);
        let mut unchanged = [0; 5];
        memory.view().read(0x46c, &mut unchanged).unwrap();
        assert_eq!(unchanged, bios_time);
    });
}

#[test]
fn time_reads_host_hundredths_and_preserves_other_registers_and_flags() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let state = dos.cpu().cpu_state();
        state.set_eax(0xabcd_2cef);
        state.set_ebx(0x1234_5678);
        state.set_ecx(0x2345_0000);
        state.set_edx(0x3456_0000);
        state.set_eflags(0x0856);
        let mut expected = state.snapshot();

        let before = Local::now().naive_local();
        let cpu = service(&mut dos, 0x2cef, 0x5678, 0, 0);
        let after = Local::now().naive_local();
        let hour = (cpu.registers.ecx >> 8) & 0xff;
        let minute = cpu.registers.ecx & 0xff;
        let second = (cpu.registers.edx >> 8) & 0xff;
        let hundredth = cpu.registers.edx & 0xff;
        assert!(hour < 24 && minute < 60 && second < 60 && hundredth < 100);
        let time =
            chrono::NaiveTime::from_hms_milli_opt(hour, minute, second, hundredth * 10).unwrap();
        // Truncate the lower bound to DOS precision and allow a midnight crossing.
        let before = before
            .with_nanosecond(before.nanosecond() / 10_000_000 * 10_000_000)
            .unwrap();
        assert!([before.date(), after.date()]
            .into_iter()
            .any(|date| (before..=after).contains(&date.and_time(time))));
        expected.registers.ecx |= (hour << 8) | minute;
        expected.registers.edx |= (second << 8) | hundredth;
        assert_eq!(cpu.registers, expected.registers);
        assert_eq!(cpu.segments, expected.segments);
        assert_eq!(cpu.flags.eflags(), 0x0856);
    });
}

fn setting(dos: &mut relocade_dos::Dos, function: u8, cx: u16, dx: u16, valid: bool) {
    let state = dos.cpu().cpu_state();
    let ax = u16::from_be_bytes([function, 0x42]);
    state.set_eax(0xabcd_0000 | u32::from(ax));
    state.set_ebx(0x1234_5678);
    state.set_ecx(0x2345_0000 | u32::from(cx));
    state.set_edx(0x3456_0000 | u32::from(dx));
    state.set_eflags(if valid { 0x0a57 } else { 0x0856 });
    let mut expected = state.snapshot();
    expected.registers.eax = (expected.registers.eax & !0xff) | if valid { 0 } else { 0xff };
    let cpu = service(dos, ax, 0x5678, cx, dx);
    assert_eq!(cpu.registers, expected.registers);
    assert_eq!(cpu.segments, expected.segments);
    assert_eq!(cpu.flags.eflags(), expected.flags.eflags());
}

#[test]
fn date_settings_validate_dos_years_and_calendar_dates() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        setting(&mut dos, 0x2d, 0x0c00, 0, true); // Keep the fixture away from midnight.
        for (year, month, day, weekday) in [(1980, 1, 1, 2), (2099, 12, 31, 4), (2000, 2, 29, 2)] {
            setting(&mut dos, 0x2b, year, u16::from_be_bytes([month, day]), true);
            let cpu = service(&mut dos, 0x2a00, 0, 0, 0);
            assert_eq!(cpu.registers.ecx as u16, year);
            assert_eq!(cpu.registers.edx as u16, u16::from_be_bytes([month, day]));
            assert_eq!(cpu.registers.eax as u8, weekday);
        }
        for (year, month, day) in [
            (1979, 12, 31),
            (2100, 1, 1),
            (2000, 0, 1),
            (2000, 13, 1),
            (2000, 1, 0),
            (2000, 4, 31),
            (2001, 2, 29),
        ] {
            setting(&mut dos, 0x2b, year, u16::from_be_bytes([month, day]), false);
        }
        let cpu = service(&mut dos, 0x2a00, 0, 0, 0);
        assert_eq!(cpu.registers.ecx as u16, 2000);
        assert_eq!(cpu.registers.edx as u16, 0x021d);
    });
}

#[test]
fn time_settings_validate_fields_and_persist_with_the_date_across_launches() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let memory = dos.cpu().memory();
        let bios_time = [0x12, 0x34, 0x56, 0x78, 7];
        memory.view().write(0x46c, &bios_time).unwrap();
        setting(&mut dos, 0x2d, 0x0c00, 0, true);
        setting(&mut dos, 0x2b, 2024, 0x021d, true);
        let target = NaiveDate::from_ymd_opt(2024, 2, 29)
            .unwrap()
            .and_hms_milli_opt(12, 34, 56, 780)
            .unwrap();
        let before = Local::now().naive_local();
        setting(&mut dos, 0x2d, 0x0c22, 0x384e, true);
        for (cx, dx) in [(0x1800, 0), (0x003c, 0), (0, 0x3c00), (0, 100)] {
            setting(&mut dos, 0x2d, cx, dx, false);
        }
        common::terminate(&mut dos);
        dos.launch_com(SERVICE_PROGRAM, Default::default()).unwrap();
        let date = service(&mut dos, 0x2a00, 0, 0, 0);
        let time = service(&mut dos, 0x2c00, 0, 0, 0);
        let after = Local::now().naive_local();
        let observed = NaiveDate::from_ymd_opt(
            i32::from(date.registers.ecx as u16),
            (date.registers.edx >> 8) & 0xff,
            date.registers.edx & 0xff,
        )
        .unwrap()
        .and_hms_milli_opt(
            (time.registers.ecx >> 8) & 0xff,
            time.registers.ecx & 0xff,
            (time.registers.edx >> 8) & 0xff,
            (time.registers.edx & 0xff) * 10,
        )
        .unwrap();
        assert!((target..=target + (after - before)).contains(&observed));
        assert_eq!(date.registers.eax as u8, 4); // Thursday.
        let mut unchanged = [0; 5];
        memory.view().read(0x46c, &mut unchanged).unwrap();
        assert_eq!(unchanged, bios_time);
    });
}

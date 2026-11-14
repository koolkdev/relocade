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

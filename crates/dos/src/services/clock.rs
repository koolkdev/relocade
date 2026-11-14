//! DOS date and time register mappings, sampled from the host's local clock.

use super::Services;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use relocade_machine::real_mode_interrupt::interrupt;

#[interrupt(returns {
    now => {
        cx = now.year() as u16,
        dh = now.month() as u8,
        dl = now.day() as u8,
        al = now.weekday().num_days_from_sunday() as u8,
    },
})]
pub(super) fn get_date(services: &Services<'_>) -> NaiveDateTime {
    services.dos.clock.now()
}

#[interrupt(returns {
    now => {
        ch = now.hour() as u8,
        cl = now.minute() as u8,
        dh = now.second() as u8,
        dl = (now.nanosecond() / 10_000_000) as u8,
    },
})]
pub(super) fn get_time(services: &Services<'_>) -> NaiveDateTime {
    services.dos.clock.now()
}

#[interrupt(args(year = cx, month = dh, day = dl), returns {
    true => { al = 0u8 },
    false => { al = 0xffu8 },
})]
pub(super) fn set_date(services: &Services<'_>, year: u16, month: u8, day: u8) -> bool {
    if !(1980..=2099).contains(&year) {
        return false;
    }
    let Some(date) = NaiveDate::from_ymd_opt(i32::from(year), u32::from(month), u32::from(day)) else {
        return false;
    };
    services.dos.clock.set_date(Local::now().naive_local(), date);
    true
}

#[interrupt(args(hour = ch, minute = cl, second = dh, hundredth = dl), returns {
    true => { al = 0u8 },
    false => { al = 0xffu8 },
})]
pub(super) fn set_time(
    services: &Services<'_>,
    hour: u8,
    minute: u8,
    second: u8,
    hundredth: u8,
) -> bool {
    if hundredth > 99 {
        return false;
    }
    let Some(time) = NaiveTime::from_hms_milli_opt(
        u32::from(hour),
        u32::from(minute),
        u32::from(second),
        u32::from(hundredth) * 10,
    ) else {
        return false;
    };
    services.dos.clock.set_time(Local::now().naive_local(), time);
    true
}

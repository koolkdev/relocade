//! DOS date and time register mappings, sampled from the host's local clock.

use chrono::{DateTime, Datelike, Local, Timelike};
use relocade_machine::real_mode_interrupt::interrupt;

#[interrupt(returns {
    now => {
        cx = now.year() as u16,
        dh = now.month() as u8,
        dl = now.day() as u8,
        al = now.weekday().num_days_from_sunday() as u8,
    },
})]
pub(super) fn get_date() -> DateTime<Local> {
    Local::now()
}

#[interrupt(returns {
    now => {
        ch = now.hour() as u8,
        cl = now.minute() as u8,
        dh = now.second() as u8,
        dl = (now.nanosecond() / 10_000_000) as u8,
    },
})]
pub(super) fn get_time() -> DateTime<Local> {
    Local::now()
}

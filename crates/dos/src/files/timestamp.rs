//! DOS directory timestamps are packed local time with two-second precision.

use chrono::{Datelike, NaiveDateTime, Timelike};

/// Keep guest-supplied words intact: AH=57h does not validate calendar fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FileTimestamp {
    pub(crate) time: u16,
    pub(crate) date: u16,
}

impl FileTimestamp {
    pub(super) fn from_datetime(now: NaiveDateTime) -> Self {
        // Host clock corrections can exceed the directory format's year range.
        if now.year() < 1980 {
            return Self {
                time: 0,
                date: 0x0021,
            };
        }
        if now.year() > 2107 {
            return Self {
                time: 0xbf7d,
                date: 0xff9f,
            };
        }
        Self {
            time: ((now.hour() << 11) | (now.minute() << 5) | (now.second() / 2)) as u16,
            date: ((((now.year() - 1980) as u32) << 9) | (now.month() << 5) | now.day()) as u16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_dates_pack_into_dos_words_and_discard_sub_two_second_precision() {
        for (text, time, date) in [
            ("1980-01-01T00:00:00", 0, 0x0021),
            ("2000-02-29T12:34:56", 0x645c, 0x285d),
            ("2000-02-29T12:34:57.999", 0x645c, 0x285d),
            ("2107-12-31T23:59:59.999", 0xbf7d, 0xff9f),
            ("1979-12-31T23:59:59", 0, 0x0021),
            ("2108-01-01T00:00:00", 0xbf7d, 0xff9f),
        ] {
            assert_eq!(
                FileTimestamp::from_datetime(text.parse().unwrap()),
                FileTimestamp { time, date }
            );
        }
    }
}

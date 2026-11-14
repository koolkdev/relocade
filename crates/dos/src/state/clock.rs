//! A session-local offset from the host's local wall clock.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeDelta};
use std::cell::Cell;

#[derive(Default)]
pub(crate) struct Clock {
    offset: Cell<TimeDelta>,
}

impl Clock {
    /// Each service supplies one host sample for its whole operation.
    pub(crate) fn read(&self, host: NaiveDateTime) -> NaiveDateTime {
        host + self.offset.get()
    }

    pub(crate) fn set_date(&self, host: NaiveDateTime, date: NaiveDate) {
        self.offset.set(date.and_time(self.read(host).time()) - host);
    }

    pub(crate) fn set_time(&self, host: NaiveDateTime, time: NaiveTime) {
        self.offset.set(self.read(host).date().and_time(time) - host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn datetime(text: &str) -> NaiveDateTime {
        text.parse().unwrap()
    }

    #[test]
    fn setters_preserve_the_other_component_and_time_keeps_advancing() {
        let clock = Clock::default();
        let host = datetime("2026-07-15T18:23:45.670");
        clock.set_date(host, NaiveDate::from_ymd_opt(2000, 2, 29).unwrap());
        assert_eq!(clock.read(host), datetime("2000-02-29T18:23:45.670"));
        clock.set_time(host, NaiveTime::from_hms_milli_opt(23, 59, 59, 990).unwrap());
        let later = host + TimeDelta::milliseconds(20);
        assert_eq!(clock.read(later), datetime("2000-03-01T00:00:00.010"));
        clock.set_date(later, NaiveDate::from_ymd_opt(1980, 1, 1).unwrap());
        assert_eq!(clock.read(later), datetime("1980-01-01T00:00:00.010"));
    }

    #[test]
    fn offsets_are_independent_and_follow_host_clock_changes() {
        let clock = Clock::default();
        let other = Clock::default();
        let host = datetime("2026-11-14T12:00:00");
        clock.set_date(host, NaiveDate::from_ymd_opt(2099, 12, 31).unwrap());
        assert_eq!(other.read(host), host);
        assert_eq!(
            clock.read(host + TimeDelta::days(60)),
            datetime("2100-03-01T12:00:00")
        );
        assert_eq!(
            clock.read(host - TimeDelta::hours(1)),
            datetime("2099-12-31T11:00:00")
        );
    }
}

//! Host/guest correspondence, excluding pauses and excessive execution lag.

use super::super::Timebase;
use crate::{Clock, Instant, Sleep, Timestamp};
use std::time::Duration;

const MAX_LAG: Duration = Duration::from_millis(20);

pub(super) struct HostTime {
    clock: Clock,
    guest_timebase: Timebase,
    mapping: Option<Mapping>,
    paused_at: Option<Timestamp>,
    pub(super) discarded_lag: Duration,
}

struct Mapping {
    host: Timestamp,
    guest: Timestamp,
}

impl HostTime {
    pub(super) fn new(clock: Clock, guest_timebase: Timebase) -> Self {
        Self {
            clock,
            guest_timebase,
            mapping: None,
            paused_at: None,
            discarded_lag: Duration::ZERO,
        }
    }

    pub(super) fn resume(&mut self, guest: Timestamp) {
        let now = self.clock.now();
        match &mut self.mapping {
            Some(mapping) => {
                let paused = self.paused_at.take().expect("host time was paused");
                // Both coordinates belong to the host clock. Preserve even a
                // sub-nanosecond pause without converting it through Duration.
                mapping.host.0 = mapping
                    .host
                    .0
                    .checked_add(now.0.saturating_sub(paused.0))
                    .expect("paused host correspondence fits its timeline");
            }
            None => self.mapping = Some(Mapping { host: now, guest }),
        }
    }

    pub(super) fn pause(&mut self) {
        self.paused_at = Some(self.clock.now());
    }

    /// Called only at stopped execution boundaries. Once lag exceeds the tolerance,
    /// discard its whole nanoseconds; retain any fraction in the correspondence.
    /// Guest time and device timers do not advance.
    pub(super) fn discard_lag(&mut self, guest: Timestamp) -> Option<Duration> {
        let mapped = self.host_time(guest)?;
        let lag = self.clock.duration_since(self.clock.now(), mapped);
        if lag <= MAX_LAG {
            return None;
        }
        let mapping = self.mapping.as_mut().unwrap();
        mapping.host = self.clock.add(mapping.host, lag);
        self.discarded_lag += lag;
        Some(lag)
    }

    pub(super) fn sync_deadline(&self, host_time: Option<Instant>) -> Option<Timestamp> {
        if self.paused_at.is_some() {
            return None;
        }
        let mapping = self.mapping.as_ref()?;
        let host_time = match host_time {
            Some(time) => self.clock.source.host_timestamp(time)?,
            None => self.clock.now(),
        };
        self.guest_timebase.checked_add(
            mapping.guest,
            self.clock.timebase.duration_until(host_time, mapping.host),
        )
    }

    fn host_time(&self, guest: Timestamp) -> Option<Timestamp> {
        if self.paused_at.is_some() {
            return None;
        }
        let mapping = self.mapping.as_ref()?;
        if guest >= mapping.guest {
            self.clock.checked_add(
                mapping.host,
                self.guest_timebase.duration_until(guest, mapping.guest),
            )
        } else {
            self.clock.checked_sub(
                mapping.host,
                self.guest_timebase.duration_since(mapping.guest, guest),
            )
        }
    }

    pub(super) fn sync_to_host(&self, guest: Timestamp) -> Sleep {
        self.host_time(guest)
            .map(|time| self.clock.sleep_until_if_needed(time))
            .unwrap_or_else(|| Sleep::new(async {}))
    }
}

#[cfg(test)]
mod tests;

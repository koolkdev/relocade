//! Host/guest correspondence, excluding pauses and excessive execution lag.

use crate::{Clock, Instant, Sleep};
use std::time::Duration;

const MAX_LAG: Duration = Duration::from_millis(20);

pub(super) struct HostTime {
    clock: Clock,
    mapping: Option<Mapping>,
    paused_at: Option<Instant>,
    pub(super) discarded_lag: Duration,
}

struct Mapping {
    host: Instant,
    guest: Instant,
}

impl HostTime {
    pub(super) fn new(clock: Clock) -> Self {
        Self {
            clock,
            mapping: None,
            paused_at: None,
            discarded_lag: Duration::ZERO,
        }
    }

    pub(super) fn resume(&mut self, guest: Instant) {
        let now = self.clock.now();
        match &mut self.mapping {
            Some(mapping) => {
                let paused = self.paused_at.take().expect("host time was paused");
                mapping.host += now.saturating_duration_since(paused);
            }
            None => self.mapping = Some(Mapping { host: now, guest }),
        }
    }

    pub(super) fn pause(&mut self) {
        self.paused_at = Some(self.clock.now());
    }

    /// Called only at stopped execution boundaries. Discard the entire lag once
    /// it exceeds the tolerance, without advancing guest time or device timers.
    pub(super) fn discard_lag(&mut self, guest: Instant) -> Option<Duration> {
        let mapped = self.host_time(guest)?;
        let lag = self.clock.now().saturating_duration_since(mapped);
        if lag <= MAX_LAG {
            return None;
        }
        self.mapping.as_mut().unwrap().host += lag;
        self.discarded_lag += lag;
        Some(lag)
    }

    pub(super) fn sync_deadline(&self, host_time: Option<Instant>) -> Option<Instant> {
        if self.paused_at.is_some() {
            return None;
        }
        let mapping = self.mapping.as_ref()?;
        let host_time = host_time.unwrap_or_else(|| self.clock.now());
        Some(mapping.guest + host_time.saturating_duration_since(mapping.host))
    }

    fn host_time(&self, guest: Instant) -> Option<Instant> {
        if self.paused_at.is_some() {
            return None;
        }
        let mapping = self.mapping.as_ref()?;
        if guest >= mapping.guest {
            mapping
                .host
                .checked_add(guest.duration_since(mapping.guest))
        } else {
            mapping
                .host
                .checked_sub(mapping.guest.duration_since(guest))
        }
    }

    pub(super) fn sync_to_host(&self, guest: Instant) -> Sleep {
        self.host_time(guest)
            .map(|time| self.clock.sleep_until_if_needed(time))
            .unwrap_or_else(|| Sleep::new(async {}))
    }
}

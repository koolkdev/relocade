//! Host/guest correspondence, excluding pauses and excessive execution lag.

use crate::{Clock, Instant, Sleep};
use std::{cell::Cell, rc::Rc, time::Duration};

const MAX_LAG: Duration = Duration::from_millis(20);

#[cfg(test)]
mod tests;

pub(super) struct HostTime {
    clock: Clock,
    mapping: Option<Mapping>,
    paused_at: Option<Instant>,
    discarded_lag: Rc<Cell<Duration>>,
}

struct Mapping {
    host: Instant,
    guest: Instant,
}

/// A captured host-derived deadline follows lag recovery, including before its
/// first poll. Ordinary pauses leave the captured deadline unchanged.
pub(super) struct SyncDeadline {
    origin: Instant,
    offset: Duration,
    discarded_at_capture: Duration,
    discarded_lag: Rc<Cell<Duration>>,
}

impl SyncDeadline {
    pub(super) fn at(&self) -> Instant {
        let discarded = self.discarded_lag.get() - self.discarded_at_capture;
        self.origin + self.offset.saturating_sub(discarded)
    }
}

impl HostTime {
    pub(super) fn new(clock: Clock) -> Self {
        Self {
            clock,
            mapping: None,
            paused_at: None,
            discarded_lag: Rc::new(Cell::new(Duration::ZERO)),
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
    pub(super) fn discard_lag(&mut self, guest: Instant) {
        let Some(mapped) = self.host_time(guest) else {
            return;
        };
        let lag = self.clock.now().saturating_duration_since(mapped);
        if lag > MAX_LAG {
            self.mapping.as_mut().unwrap().host += lag;
            self.discarded_lag.set(self.discarded_lag.get() + lag);
        }
    }

    pub(super) fn sync_deadline(&self, host_time: Option<Instant>) -> Option<SyncDeadline> {
        if self.paused_at.is_some() {
            return None;
        }
        let mapping = self.mapping.as_ref()?;
        let host_time = host_time.unwrap_or_else(|| self.clock.now());
        Some(SyncDeadline {
            origin: mapping.guest,
            offset: host_time.saturating_duration_since(mapping.host),
            discarded_at_capture: self.discarded_lag.get(),
            discarded_lag: self.discarded_lag.clone(),
        })
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

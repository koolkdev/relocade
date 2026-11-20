//! Host/guest correspondence, excluding pauses and excessive execution lag.

use crate::{Clock, Sleep, TickRate, Timestamp};
use std::time::Duration;

const MAX_LAG: Duration = Duration::from_millis(20);

pub(super) struct HostTime {
    clock: Clock,
    guest_rate: TickRate,
    mapping: Option<Mapping>,
    paused_at: Option<Timestamp>,
}

struct Mapping {
    host: Timestamp,
    guest: Timestamp,
}

impl HostTime {
    pub(super) fn new(clock: Clock, guest_rate: TickRate) -> Self {
        Self {
            clock,
            guest_rate,
            mapping: None,
            paused_at: None,
        }
    }

    pub(super) fn resume(&mut self, guest: Timestamp) {
        let now = self.clock.now();
        match &mut self.mapping {
            Some(mapping) => {
                let paused = self.paused_at.take().expect("host time was paused");
                // Both coordinates belong to the host clock. Preserve even a
                // sub-nanosecond pause without converting it through Duration.
                mapping.host = Timestamp::from_ticks(
                    mapping
                        .host
                        .ticks()
                        .wrapping_add(now.ticks().wrapping_sub(paused.ticks())),
                );
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
    pub(super) fn discard_lag(&mut self, guest: Timestamp) {
        let Some(mapped) = self.host_time(guest) else {
            return;
        };
        let lag = self.clock.duration_between(mapped, self.clock.now());
        if lag <= MAX_LAG {
            return;
        }
        let mapping = self.mapping.as_mut().unwrap();
        mapping.host = self.clock.deadline_after(mapping.host, lag);
    }

    fn host_time(&self, guest: Timestamp) -> Option<Timestamp> {
        if self.paused_at.is_some() {
            return None;
        }
        let mapping = self.mapping.as_ref()?;
        if guest >= mapping.guest {
            self.clock.checked_deadline_after(
                mapping.host,
                self.guest_rate.duration_between_ceil(mapping.guest, guest),
            )
        } else {
            self.clock.checked_deadline_before(
                mapping.host,
                self.guest_rate.duration_between(guest, mapping.guest),
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

use super::{Instant, SystemTime};

/// A calendar anchor, independent of clock advancement and deadline scheduling.
/// Mapping follows this snapshot rather than later host clock corrections.
#[derive(Clone, Copy, Debug)]
pub struct TimeOrigin {
    instant: Instant,
    system_time: SystemTime,
}

impl TimeOrigin {
    pub fn new(instant: Instant, system_time: SystemTime) -> Self {
        Self {
            instant,
            system_time,
        }
    }

    /// Samples the host monotonic and calendar clocks for a session.
    pub fn now() -> Self {
        Self::new(Instant::now(), SystemTime::now())
    }

    pub fn instant(&self) -> Instant {
        self.instant
    }

    /// Converts instants before or after the anchor to calendar time.
    /// Panics if the result is outside the platform's SystemTime range.
    pub fn to_system_time(&self, instant: Instant) -> SystemTime {
        if instant >= self.instant {
            self.system_time + instant.duration_since(self.instant)
        } else {
            self.system_time - self.instant.duration_since(instant)
        }
    }
}

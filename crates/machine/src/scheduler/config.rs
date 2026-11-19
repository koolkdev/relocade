//! Configuration for the execution owner's emulated time.

use crate::Clock;
use std::num::NonZeroU64;

/// Device time advances one tick per retired instruction at the nominal guest rate.
/// A host clock can time idle devices and output. A separate limit caps CPU work.
#[derive(Clone)]
pub struct GuestTimeConfig {
    pub(super) instructions_per_second: NonZeroU64,
    pub(super) host_clock: Option<Clock>,
    pub(super) cpu_limit: Option<NonZeroU64>,
}

impl GuestTimeConfig {
    /// Runs at full CPU speed and advances idle time directly to the next guest deadline.
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self {
            instructions_per_second,
            host_clock: None,
            cpu_limit: None,
        }
    }

    /// Times idle device deadlines and presents output against a live host clock.
    /// Guest execution remains unrestricted unless a CPU limit is configured.
    pub fn with_host_clock(mut self, clock: Clock) -> Self {
        self.host_clock = Some(clock);
        self
    }

    /// Limits retired instructions per host second, independently of the guest clock rate.
    /// The host clock also supplies idle timing and output synchronization.
    pub fn with_cpu_limit(mut self, instructions_per_second: NonZeroU64, clock: Clock) -> Self {
        self.cpu_limit = Some(instructions_per_second);
        self.with_host_clock(clock)
    }
}

//! Guest timing policy passed to the execution worker.

use std::num::NonZeroU64;

/// A live device clock tracks elapsed host time at the chosen tick resolution.
/// An optional throughput limit caps retired instructions per host second.
#[derive(Clone, Copy, Debug)]
pub struct GuestTimeConfig {
    pub(super) ticks_per_second: NonZeroU64,
    pub(super) cpu_limit: Option<NonZeroU64>,
}

impl GuestTimeConfig {
    /// Sets the guest clock rate and leaves CPU throughput unrestricted.
    pub const fn new(ticks_per_second: NonZeroU64) -> Self {
        Self {
            ticks_per_second,
            cpu_limit: None,
        }
    }

    /// Limits retired instructions per host second independently of the guest clock rate.
    pub const fn with_cpu_limit(mut self, instructions_per_second: NonZeroU64) -> Self {
        self.cpu_limit = Some(instructions_per_second);
        self
    }
}

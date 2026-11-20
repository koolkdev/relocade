//! One linear estimate between samples of the host clock and CPU retirement.

use crate::Timestamp;

// A fixed-point guest tick keeps fractional time across changes of slope.
pub(super) const ONE_TICK: u128 = 1 << 32;

#[derive(Default)]
pub(super) struct Interpolation {
    pub(super) retired: u64,
    phase: u128,
    step: u128,
    span: u128,
    instructions: u64,
}

impl Interpolation {
    pub(super) fn phase(&self, retired: u64) -> u128 {
        let count = retired.wrapping_sub(self.retired).min(self.instructions);
        self.phase + (u128::from(count) * self.step).min(self.span)
    }

    pub(super) fn now(&self, retired: u64) -> Timestamp {
        Timestamp::from_ticks((self.phase(retired) >> 32) as u64)
    }

    /// Only deadlines reachable before the next sample need a CPU target.
    pub(super) fn target(&self, deadline: Timestamp, retired: u64) -> Option<u64> {
        if deadline <= self.now(retired) {
            return Some(retired);
        }
        let distance = deadline.ticks().wrapping_sub((self.phase >> 32) as u64);
        let remaining = u128::from(distance) * ONE_TICK - (self.phase % ONE_TICK);
        if remaining > self.span || self.step == 0 {
            return None;
        }
        let count = remaining.div_ceil(self.step);
        Some(
            self.retired
                .wrapping_add(count.min(i64::MAX as u128) as u64),
        )
    }

    /// Aim for the next wall-clock sample. A late sample can recover up to
    /// one extra window of lag without jumping over previously issued ticks.
    pub(super) fn anchor(
        &mut self,
        retired: u64,
        phase: u128,
        wall: u128,
        window: u128,
        instructions: u64,
    ) {
        self.retired = retired;
        self.phase = phase;
        self.instructions = instructions;
        self.span = (wall + window).saturating_sub(phase).min(2 * window);
        self.step = self.span.div_ceil(u128::from(instructions));
    }
}

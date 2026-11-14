//! Reusable synchronous deadlines, prepared by their clock source.

use super::{ClockSource, Timestamp};
use std::{cell::Cell, rc::Rc};

/// A clock-owned deadline for repeated synchronous checks.
/// Setting a deadline lets the source prepare its native comparison. Checking
/// returns the current timestamp only once it is due. This registers no sleep
/// and does not wake a task; the caller owns polling and rearming.
pub trait ClockDeadline {
    /// The configured absolute deadline on this source's timeline.
    fn get(&self) -> Option<Timestamp>;

    /// Replaces the deadline. `None` disables polling without reading the clock.
    fn set(&self, deadline: Option<Timestamp>);

    /// Returns the observation at or after the deadline, or `None` before it.
    /// The deadline remains armed until the caller replaces it.
    fn now_if_due(&self) -> Option<Timestamp>;
}

pub(super) struct Source<C: ClockSource + ?Sized> {
    pub(super) clock: Rc<C>,
    pub(super) deadline: Cell<Option<Timestamp>>,
}

impl<C: ClockSource + ?Sized> ClockDeadline for Source<C> {
    fn get(&self) -> Option<Timestamp> {
        self.deadline.get()
    }

    fn set(&self, deadline: Option<Timestamp>) {
        self.deadline.set(deadline);
    }

    fn now_if_due(&self) -> Option<Timestamp> {
        let deadline = self.deadline.get()?;
        let now = self.clock.now();
        (now >= deadline).then_some(now)
    }
}

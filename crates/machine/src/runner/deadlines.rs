//! Hardware transitions due on the session clock.
use super::schedule::ExecutionSchedule;
use crate::{Sleep, Timestamp};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Waker},
};

/// Reprograms a registered hardware transition, for example from a port write.
pub struct DeviceDeadline {
    at: Rc<Cell<Option<Timestamp>>>,
    session: Weak<RefCell<Changes>>,
}

impl DeviceDeadline {
    /// `None` disarms the transition. Changing it wakes the session so a running
    /// CPU can stop at a checkpoint and receive its new instruction target.
    pub fn set(&self, at: Option<Timestamp>) {
        if self.at.replace(at) == at {
            return;
        }
        if let Some(session) = self.session.upgrade() {
            let mut changes = session.borrow_mut();
            changes.dirty = true;
            if let Some(waker) = &changes.waker {
                waker.wake_by_ref();
            }
        }
    }
}

#[derive(Default)]
struct Changes {
    waker: Option<Waker>,
    dirty: bool,
}

struct Source {
    at: Rc<Cell<Option<Timestamp>>>,
    advance: Box<dyn FnMut(Timestamp) -> Option<Timestamp>>,
}

#[derive(Default)]
pub(super) struct DeviceDeadlines {
    schedule: Option<Rc<ExecutionSchedule>>,
    next: Option<Timestamp>,
    sources: Vec<Source>,
    session: Rc<RefCell<Changes>>,
    external_wait: Option<Sleep>,
}

impl DeviceDeadlines {
    pub(super) fn register(
        &mut self,
        schedule: &Rc<ExecutionSchedule>,
        initial: Option<Timestamp>,
        advance: impl FnMut(Timestamp) -> Option<Timestamp> + 'static,
    ) -> DeviceDeadline {
        self.schedule.get_or_insert_with(|| schedule.clone());
        let at = Rc::new(Cell::new(None));
        self.sources.push(Source {
            at: at.clone(),
            advance: Box::new(advance),
        });
        let deadline = DeviceDeadline {
            at,
            session: Rc::downgrade(&self.session),
        };
        deadline.set(initial);
        deadline
    }

    pub(super) fn poll(&mut self, cx: &mut Context<'_>) {
        let dirty = {
            let mut changes = self.session.borrow_mut();
            changes
                .waker
                .get_or_insert_with(|| cx.waker().clone())
                .clone_from(cx.waker());
            std::mem::take(&mut changes.dirty)
        };
        let Some(schedule) = &self.schedule else {
            return;
        };
        let clock = schedule.clock();
        let now = clock.now();
        if dirty || self.next.is_some_and(|at| at <= now) {
            // All devices in a turn observe one timestamp. Dispatch at most once
            // per device, even when several edges elapsed before the checkpoint.
            for source in &mut self.sources {
                if source.at.get().is_some_and(|at| at <= now) {
                    source.at.set((source.advance)(now));
                }
            }

            let next = self
                .sources
                .iter()
                .filter_map(|source| source.at.get())
                .min();
            if next != self.next {
                self.next = next;
                if let Some(driver) = schedule.guest_driver() {
                    driver.set_hardware_deadline(next);
                } else {
                    // An independently driven clock must wake a parked session.
                    self.external_wait = next.map(|at| clock.sleep_until(at));
                }
            }
            if next.is_some_and(|at| at <= now) {
                cx.waker().wake_by_ref();
            }
        }

        if let Some(wait) = &mut self.external_wait {
            if Pin::new(wait).poll(cx).is_ready() {
                self.external_wait = None;
                cx.waker().wake_by_ref();
            }
        }
    }
}

impl Drop for DeviceDeadlines {
    fn drop(&mut self) {
        if let Some(driver) = self.schedule.as_ref().and_then(|s| s.guest_driver()) {
            driver.set_hardware_deadline(None);
        }
    }
}

#[cfg(test)]
mod tests;

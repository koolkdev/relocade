//! Owned guest sleeps register their task directly on the clock's deadline list.

use super::State;
use crate::{ClockSource, Sleep, Timestamp};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

#[derive(Clone)]
pub(super) struct Source(pub(super) Rc<RefCell<State>>);

pub(super) struct Wait {
    pub(super) deadline: Timestamp,
    pub(super) waker: RefCell<Option<Waker>>,
}

#[derive(Default)]
pub(super) struct Waits {
    entries: Vec<Rc<Wait>>,
    pub(super) earliest: Option<Timestamp>,
}

impl Waits {
    fn insert(&mut self, wait: Rc<Wait>) {
        self.earliest = Some(
            self.earliest
                .map_or(wait.deadline, |at| at.min(wait.deadline)),
        );
        self.entries.push(wait);
    }

    fn cancel(&mut self, wait: &Rc<Wait>) {
        self.entries.retain(|entry| !Rc::ptr_eq(entry, wait));
        if self.earliest == Some(wait.deadline) {
            self.earliest = self.entries.iter().map(|entry| entry.deadline).min();
        }
    }

    pub(super) fn take_due(&mut self, now: Timestamp) -> Vec<Waker> {
        let mut due = Vec::new();
        if self.earliest.is_some_and(|at| at <= now) {
            self.earliest = None;
            self.entries.retain(|wait| {
                if wait.deadline <= now {
                    due.push(wait.waker.take().expect("registered guest sleep"));
                    false
                } else {
                    self.earliest = Some(
                        self.earliest
                            .map_or(wait.deadline, |at| at.min(wait.deadline)),
                    );
                    true
                }
            });
        }
        due
    }
}

struct GuestSleep {
    source: Source,
    deadline: Timestamp,
    registered: Option<Rc<Wait>>,
}

impl Future for GuestSleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(wait) = &self.registered {
            if let Some(waker) = wait.waker.borrow_mut().as_mut() {
                waker.clone_from(cx.waker());
                return Poll::Pending;
            }
            return Poll::Ready(());
        }
        if self.deadline <= self.source.0.borrow().now() {
            return Poll::Ready(());
        }
        let wait = Rc::new(Wait {
            deadline: self.deadline,
            waker: RefCell::new(Some(cx.waker().clone())),
        });
        self.registered = Some(wait.clone());
        self.source.update_waits(|waits| waits.insert(wait));
        Poll::Pending
    }
}

impl Drop for GuestSleep {
    fn drop(&mut self) {
        let Some(wait) = &self.registered else { return };
        if wait.waker.borrow().is_some() {
            self.source.update_waits(|waits| waits.cancel(wait));
        }
    }
}

impl Source {
    fn update_waits(&self, update: impl FnOnce(&mut Waits)) {
        let changed = {
            let mut state = self.0.borrow_mut();
            let previous = state.waits.earliest;
            update(&mut state.waits);
            if previous != state.waits.earliest {
                state.refresh_target();
            } else if state.realtime.is_some() {
                return;
            }
            state.cpu.as_ref().map(|cpu| cpu.changed.clone())
        };
        if let Some(changed) = changed {
            changed.wake();
        }
    }
}

impl ClockSource for Source {
    fn now(&self) -> Timestamp {
        self.0.borrow().now()
    }

    fn ticks_per_second(&self) -> std::num::NonZeroU64 {
        self.0.borrow().tick_rate.ticks_per_second()
    }

    fn sync_to_host(&self, time: Timestamp) -> Sleep {
        let state = self.0.borrow();
        if let (Some(realtime), Some(_)) = (&state.realtime, &state.cpu) {
            return realtime.sleep_until(time);
        }
        Sleep::new(async {})
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        Sleep::new(GuestSleep {
            source: self.clone(),
            deadline,
            registered: None,
        })
    }
}

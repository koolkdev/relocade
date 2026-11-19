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
        if self.deadline <= self.source.now() {
            return Poll::Ready(());
        }
        let wait = Rc::new(Wait {
            deadline: self.deadline,
            waker: RefCell::new(Some(cx.waker().clone())),
        });
        self.registered = Some(wait.clone());
        self.source.update_waits(|waits| waits.push(wait));
        Poll::Pending
    }
}

impl Drop for GuestSleep {
    fn drop(&mut self) {
        let Some(wait) = &self.registered else { return };
        if wait.waker.borrow().is_some() {
            self.source
                .update_waits(|waits| waits.retain(|entry| !Rc::ptr_eq(entry, wait)));
        }
    }
}

impl Source {
    fn update_waits(&self, update: impl FnOnce(&mut Vec<Rc<Wait>>)) {
        let changed = {
            let mut state = self.0.borrow_mut();
            update(&mut state.waits);
            state.refresh_target();
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
        self.0
            .borrow()
            .host
            .as_ref()
            .map(|host| host.sync_to_host(time))
            .unwrap_or_else(|| Sleep::new(async {}))
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        Sleep::new(GuestSleep {
            source: self.clone(),
            deadline,
            registered: None,
        })
    }
}

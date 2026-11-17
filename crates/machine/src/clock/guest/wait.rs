//! Owned sleep registration and synchronous cancellation.

use super::State;
use crate::{ClockSource, Sleep, Timestamp};
use futures_channel::oneshot;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
pub(super) struct Source(pub(super) Rc<RefCell<State>>);

pub(super) struct Wait {
    pub(super) token: Rc<()>,
    pub(super) deadline: Timestamp,
    pub(super) complete: oneshot::Sender<()>,
}

// An owned registration removes its wait and refreshes the CPU target at drop,
// including cancellation from inside an executing IO/MMIO callback.
struct Registration {
    source: Source,
    token: Rc<()>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let (removed, changed) = {
            let mut state = self.source.0.borrow_mut();
            let Some(index) = state
                .waits
                .iter()
                .position(|wait| Rc::ptr_eq(&wait.token, &self.token))
            else {
                return;
            };
            let removed = state.waits.swap_remove(index);
            state.refresh_target();
            (removed, state.cpu.as_ref().map(|cpu| cpu.changed.clone()))
        };
        drop(removed);
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
        let source = self.clone();
        Sleep::new(async move {
            let now = source.now();
            if deadline <= now {
                return;
            }
            let (complete, completed) = oneshot::channel();
            let token = Rc::new(());
            let registration = Registration {
                source,
                token: token.clone(),
            };
            let changed = {
                let mut state = registration.source.0.borrow_mut();
                state.waits.push(Wait {
                    token,
                    deadline,
                    complete,
                });
                state.refresh_target();
                state.cpu.as_ref().map(|cpu| cpu.changed.clone())
            };
            if let Some(changed) = changed {
                changed.wake();
            }
            completed
                .await
                .expect("guest time driver owns pending sleeps");
            drop(registration);
        })
    }
}

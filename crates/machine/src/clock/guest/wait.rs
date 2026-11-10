//! Owned sleep registration and synchronous cancellation.

use super::State;
use crate::{ClockSource, Instant, Sleep, Timestamp};
use futures_channel::oneshot;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Clone)]
pub(super) struct Source(pub(super) Rc<RefCell<State>>);

pub(super) struct Wait {
    pub(super) token: Rc<()>,
    pub(super) deadline: Timestamp,
    pub(super) host_sync: bool,
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
        self.0.borrow_mut().observe()
    }

    fn sync_to_host(&self, time: Timestamp) -> Sleep {
        self.0
            .borrow()
            .host
            .as_ref()
            .map(|host| host.sync_to_host(time))
            .unwrap_or_else(|| Sleep::new(async {}))
    }

    fn sync(&self, host_time: Option<Instant>) -> Sleep {
        let state = self.0.borrow();
        let Some(host) = &state.host else {
            return Sleep::new(async {});
        };
        match host.sync_deadline(host_time) {
            Some(deadline) => self.wait(deadline, Some(host.discarded_lag)),
            None => Sleep::new(async {}),
        }
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        self.wait(deadline, None)
    }
}

impl Source {
    fn wait(&self, mut deadline: Timestamp, captured_lag: Option<Duration>) -> Sleep {
        let source = self.clone();
        Sleep::new(async move {
            let now = source.now();
            if let Some(captured) = captured_lag {
                // Recovery can happen between constructing and polling this wait.
                let discarded = source.0.borrow().host.as_ref().unwrap().discarded_lag - captured;
                deadline = source
                    .0
                    .borrow()
                    .timebase
                    .checked_sub(deadline, discarded)
                    .unwrap_or(now);
            }
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
                    host_sync: captured_lag.is_some(),
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

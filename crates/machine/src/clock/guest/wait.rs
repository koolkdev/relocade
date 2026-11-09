//! Owned sleep registration and synchronous cancellation.

use super::State;
use crate::{ClockSource, Instant, Sleep};
use futures_channel::oneshot;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
pub(super) struct Source(pub(super) Rc<RefCell<State>>);

pub(super) struct Wait {
    pub(super) token: Rc<()>,
    pub(super) deadline: Instant,
    pub(super) complete: oneshot::Sender<()>,
}

// Removing an owned wait notifies the embedding to synchronize the target
// before it resumes execution.
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
            (removed, state.cpu.as_ref().map(|cpu| cpu.changed.clone()))
        };
        drop(removed);
        if let Some(changed) = changed {
            changed.wake();
        }
    }
}

impl ClockSource for Source {
    fn now(&self) -> Instant {
        self.0.borrow_mut().observe()
    }

    fn sleep_until(&self, deadline: Instant) -> Sleep {
        let source = self.clone();
        Sleep::new(async move {
            if deadline <= source.now() {
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

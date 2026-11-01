//! Cooperative stop requests belong to the currently active run.

use std::{cell::Cell, rc::Rc, task::Waker};

/// Requests a stop at the next driver boundary and wakes a pending run.
/// Capture this with a service's other resources. It retains no CPU or backend.
/// The current guest execution or host poll finishes before the request is
/// observed, including a returning handler's normal cleanup.
///
/// A stop observed by the driver drops its pending call and returns
/// `ExecutionExit::Host(HostExit::Return)`. Requests outside an active run are
/// ignored. Returning from or dropping a run clears its request and waker;
/// dropping the waiter alone preserves the call for a replacement run.
#[derive(Clone)]
pub struct StopHandle(Rc<State>);

#[derive(Default)]
struct State {
    active: Cell<bool>,
    requested: Cell<bool>,
    waker: Cell<Option<Waker>>,
}

impl StopHandle {
    pub(super) fn new() -> Self {
        Self(Rc::default())
    }

    /// Stops the current run cooperatively. Repeated requests coalesce.
    pub fn stop(&self) {
        if self.0.active.get() {
            self.0.requested.set(true);
            if let Some(waker) = self.0.waker.take() {
                waker.wake();
            }
        }
    }

    pub(super) fn enter(&self) -> Running<'_> {
        debug_assert!(!self.0.active.get());
        self.0.active.set(true);
        Running(&self.0)
    }
}

pub(super) struct Running<'a>(&'a State);

impl Running<'_> {
    #[inline]
    pub(super) fn requested(&self) -> bool {
        self.0.requested.get()
    }

    pub(super) fn register(&self, waker: &Waker) {
        // Waker cloning and destruction may run executor code. No shared slot
        // borrow remains held while either operation executes.
        let previous = self.0.waker.replace(Some(waker.clone()));
        drop(previous);
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.active.set(false);
        self.0.requested.set(false);
        self.0.waker.take();
    }
}

//! Execution-local requests to return from the CPU driver.

use std::{cell::Cell, rc::Rc, task::Waker};

/// Requests a CPU run to stop at a host boundary. Capture this once when
/// registering a service; the handle does not retain the CPU or its backend.
/// A pending run is woken, but synchronous guest or host code is not preempted.
/// The driver consumes the request, releases pending calls, and returns
/// `ExecutionExit::Host(HostExit::Return)` without executing service completion.
/// DOS process results remain owned by DOS. This is separate from guest HLT
/// and from shutting down the runner's device activities.
#[derive(Clone)]
pub struct StopHandle(Rc<State>);

#[derive(Default)]
struct State {
    requested: Cell<bool>,
    waker: Cell<Option<Waker>>,
}

impl StopHandle {
    pub(super) fn new() -> Self {
        Self(Rc::default())
    }

    /// Requests a stop and wakes the pending run, if any. Repeated requests
    /// coalesce. A zero-budget run leaves the request pending for a later run.
    pub fn stop(&self) {
        self.0.requested.set(true);
        let waker = self.0.waker.take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    #[inline]
    pub(super) fn requested(&self) -> bool {
        self.0.requested.get()
    }

    pub(super) fn register(&self, waker: &Waker) {
        // Clone and destroy wakers without borrowing their shared slot. Either
        // operation can invoke executor code, including another stop request.
        let previous = self.0.waker.replace(Some(waker.clone()));
        drop(previous);
    }

    pub(super) fn clear_waker(&self) {
        self.0.waker.take();
    }

    pub(super) fn clear(&self) {
        self.clear_waker();
        self.0.requested.set(false);
    }

    pub(super) fn waiter(&self) -> impl Drop + '_ {
        struct Waiter<'a>(&'a StopHandle);
        impl Drop for Waiter<'_> {
            fn drop(&mut self) {
                self.0.clear_waker();
            }
        }
        Waiter(self)
    }
}

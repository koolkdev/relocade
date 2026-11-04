//! Host resources that must finish before the next guest entry.

use std::{
    cell::{Cell, RefCell},
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

/// Awaits host work that can complete independently of guest execution.
///
/// While pending inside an [`super::Executor`], this prevents its CPU from
/// entering guest code. Rust services and device activities continue to run,
/// allowing the executor to park for browser compilation or resource delivery.
/// Ready work stays synchronous. Outside an executor this is an ordinary await.
///
/// Do not wrap input waits or worker tasks that need the guest to make progress:
/// those would deadlock. Annotate the independent resource operation itself.
/// Each caller of a shared future must wrap its own await.
/// The future must remain pollable while guest execution is paused: use an
/// application/device activity or retained async service, not a synchronous
/// guest or IRQ callback as its polling owner.
pub async fn wait_for_host<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut registration: Option<Registration> = None;
    poll_fn(|cx| {
        let result = future.as_mut().poll(cx);
        let owner = if result.is_pending() {
            CURRENT.with_borrow(Clone::clone)
        } else {
            None
        };
        if registration.as_ref().map(|r| Rc::as_ptr(&r.0)) != owner.as_ref().map(Rc::as_ptr) {
            // Release the old owner if a retained future moves between executors.
            registration = owner.map(Registration::new);
        }
        result
    })
    .await
}

#[derive(Default)]
pub(super) struct HostWaits {
    pending: Cell<usize>,
    execution: Cell<bool>,
    waiter: Cell<Option<Waker>>,
}

impl HostWaits {
    pub(super) fn enter(self: &Rc<Self>) -> PollScope {
        PollScope(CURRENT.replace(Some(self.clone())))
    }

    pub(super) fn execution_waiter(&self) -> ExecutionWaiter<'_> {
        assert!(!self.execution.replace(true), "one CPU waiter per executor");
        ExecutionWaiter(self)
    }
}

/// Registers the CPU's wait for independent host resources before guest entry.
/// Retain one for the duration of a CPU run, including across pending services.
/// Dropping it releases the task waker, so a replacement run can register its own.
pub struct ExecutionWaiter<'a>(&'a HostWaits);

impl ExecutionWaiter<'_> {
    /// Check after polling retained Rust services and before parking them for
    /// guest execution. A service may itself be awaiting a host resource.
    pub fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if self.0.pending.get() == 0 {
            return Poll::Ready(());
        }
        let previous = self.0.waiter.replace(Some(cx.waker().clone()));
        drop(previous);
        Poll::Pending
    }
}

impl Drop for ExecutionWaiter<'_> {
    fn drop(&mut self) {
        let previous = self.0.waiter.take();
        self.0.execution.set(false);
        drop(previous);
    }
}

struct Registration(Rc<HostWaits>);

impl Registration {
    fn new(owner: Rc<HostWaits>) -> Self {
        owner.pending.set(owner.pending.get() + 1);
        Self(owner)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let pending = self.0.pending.get() - 1;
        self.0.pending.set(pending);
        if pending == 0 {
            // Wake the CPU child, including its enclosing combinator's queue.
            // Invoking a waker or its destructor may reenter this owner.
            let waiter = self.0.waiter.take();
            if let Some(waiter) = waiter {
                waiter.wake();
            }
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Rc<HostWaits>>> = const { RefCell::new(None) };
}

pub(super) struct PollScope(Option<Rc<HostWaits>>);

impl Drop for PollScope {
    fn drop(&mut self) {
        CURRENT.replace(self.0.take());
    }
}

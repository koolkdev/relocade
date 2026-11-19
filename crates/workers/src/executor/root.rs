//! A single composed future can contain synchronous code before a ready sibling.

use super::{Executor, WakeTarget, WorkerWake};
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

impl<R: WakeTarget> Executor<R> {
    /// Runs a composed root future until it completes or `stop` resolves.
    /// A sibling which enters synchronous code first observes the interrupt
    /// that made the root runnable, including wakes during its previous poll.
    pub async fn run_until<S: Future, F: Future>(&self, stop: S, future: F) -> Option<F::Output> {
        let mut future = pin!(future);
        let root = Arc::new(RootWake {
            worker: self.wake.clone(),
            interrupted: AtomicBool::new(false),
        });
        let waker = Waker::from(root.clone());
        self.dispatch(
            stop,
            || true,
            || {
                if root.interrupted.swap(false, Ordering::SeqCst) {
                    self.wake.interrupt.wake_by_ref();
                }
                future.as_mut().poll(&mut Context::from_waker(&waker))
            },
        )
        .await
    }

    /// Yields inside a composed root without requesting an interrupt. The
    /// enclosing combinator still receives the child's ordinary notification.
    pub async fn yield_now(&self) {
        let mut yielded = false;
        poll_fn(|cx| {
            if std::mem::replace(&mut yielded, true) {
                Poll::Ready(())
            } else {
                let _scope = YieldScope(YIELDING.replace(Arc::as_ptr(&self.wake)));
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    }
}

thread_local! {
    static YIELDING: Cell<*const WorkerWake> = const { Cell::new(std::ptr::null()) };
}

struct YieldScope(*const WorkerWake);
impl Drop for YieldScope {
    fn drop(&mut self) {
        YIELDING.set(self.0);
    }
}

struct RootWake {
    worker: Arc<WorkerWake>,
    interrupted: AtomicBool,
}

impl Wake for RootWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if YIELDING.get() == Arc::as_ptr(&self.worker) {
            YIELDING.set(std::ptr::null());
            self.worker.schedule();
        } else {
            self.interrupted.store(true, Ordering::SeqCst);
            self.worker.wake_by_ref();
        }
    }
}

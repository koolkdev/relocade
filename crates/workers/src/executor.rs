//! Polling and parking for a worker's root future.

use super::{WakeConnection, WakePeer, WakeTarget};
use crate::host_turn::HostTurns;
use futures_timer::Delay;
use futures_util::task::AtomicWaker;
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

/// Owns polling, cancellation and parking for a worker's root future.
/// Application and device futures are composed into one root future. Runnable
/// work is repolled here; idle waits and requested host turns use the caller's executor.
pub struct Executor<R: WakeTarget> {
    connection: WakeConnection<R>,
    wake: Arc<ExecutorWake>,
    pub(crate) host_turns: Rc<HostTurns>,
}

impl<R: WakeTarget> Executor<R> {
    /// Captures the signal binding for an additional worker's setup.
    pub fn peer(&self) -> WakePeer<R> {
        self.connection.peer()
    }

    pub fn new(connection: WakeConnection<R>) -> Self {
        let wake = Arc::new(ExecutorWake {
            interrupt: connection.waker().clone(),
            parked: AtomicWaker::new(),
            runnable: AtomicBool::new(false),
            notified: AtomicBool::new(false),
        });
        let host_turns = Rc::new(HostTurns::new(Waker::from(wake.clone())));
        Self {
            connection,
            wake,
            host_turns,
        }
    }

    /// Lets runnable work continue for `interval` before each pending delivery turn.
    /// The timer must progress independently of this executor and guest time.
    /// Without a timer, pending posted messages request a host turn immediately.
    /// Configure before binding receivers with [`crate::Receiver::with_executor`].
    pub fn with_host_timer<F: Future<Output = ()> + 'static>(
        mut self,
        interval: Duration,
        timer: impl Fn(Duration) -> F + 'static,
    ) -> Self {
        self.host_turns =
            Rc::new(HostTurns::new(Waker::from(self.wake.clone())).with_timer(interval, timer));
        self
    }

    /// Runs the worker's root future until it completes or stop resolves.
    /// Stop is polled first, registering its waker before application code can
    /// enter synchronous guest execution. Cancellation returns None and drops
    /// the root future on its owner. Run one root at a time with this executor.
    pub async fn run_until<S: Future, F: Future>(&self, stop: S, future: F) -> Option<F::Output> {
        let mut stop = pin!(stop);
        let mut future = pin!(future);
        let waker = Waker::from(self.wake.clone());
        loop {
            self.wake.runnable.store(false, Ordering::SeqCst);
            self.wake.notified.store(false, Ordering::SeqCst);
            let mut context = Context::from_waker(&waker);
            if stop.as_mut().poll(&mut context).is_ready() {
                return None;
            }
            let result = future.as_mut().poll(&mut context);
            // Keep the signal through the whole poll: CPU-first composition
            // must yield before its ready siblings can consume their input.
            self.connection.acknowledge();
            // A wake after a sibling was polled still needs another checkpoint.
            // Record before requesting, and acknowledge before checking, so a
            // wake racing with acknowledgement cannot be lost.
            if self.wake.notified.load(Ordering::SeqCst) {
                self.wake.interrupt.wake_by_ref();
            }
            if let Poll::Ready(value) = result {
                return Some(value);
            }
            if self.host_turns.has_waiters() {
                // Gate the whole root, including busy device futures, for one
                // host turn. Guest execution resumes even if delivery is late.
                let mut turn = pin!(Delay::new(Duration::ZERO));
                let _registration = ParkRegistration(&self.wake.parked);
                let stopped = poll_fn(|cx| {
                    self.wake.parked.register(cx.waker());
                    if stop
                        .as_mut()
                        .poll(&mut Context::from_waker(&waker))
                        .is_ready()
                    {
                        return Poll::Ready(true);
                    }
                    turn.as_mut().poll(cx).map(|()| false)
                })
                .await;
                if stopped {
                    return None;
                }
            }
            self.host_turns.resume();
            if self.wake.runnable.load(Ordering::SeqCst) {
                continue;
            }
            // Register only while idle. Recheck after registering so a wake
            // between the readiness check and registration cannot be lost.
            let _registration = ParkRegistration(&self.wake.parked);
            poll_fn(|cx| {
                if self.wake.runnable.load(Ordering::SeqCst) {
                    return Poll::Ready(());
                }
                self.wake.parked.register(cx.waker());
                if self.wake.runnable.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
    }

    /// Yields busy execution without requesting another interrupt. The current
    /// task's waker still runs, including any enclosing combinator's bookkeeping.
    pub async fn yield_now(&self) {
        let mut yielded = false;
        poll_fn(|cx| {
            if std::mem::replace(&mut yielded, true) {
                Poll::Ready(())
            } else {
                // Suppress only the first root notification from this child wake.
                // Any additional notification remains an interrupt request.
                let _scope = YieldScope(YIELDING.replace(Arc::as_ptr(&self.wake)));
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    }
}

thread_local! {
    static YIELDING: Cell<*const ExecutorWake> = const { Cell::new(std::ptr::null()) };
}

struct YieldScope(*const ExecutorWake);
impl Drop for YieldScope {
    fn drop(&mut self) {
        YIELDING.set(self.0);
    }
}

struct ExecutorWake {
    interrupt: Waker,
    parked: AtomicWaker,
    runnable: AtomicBool,
    // Interrupting wakes during a root poll must survive its acknowledgement.
    notified: AtomicBool,
}

impl Wake for ExecutorWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if YIELDING.get() == Arc::as_ptr(self) {
            YIELDING.set(std::ptr::null());
        } else {
            self.notified.store(true, Ordering::SeqCst);
            self.interrupt.wake_by_ref();
        }
        self.runnable.store(true, Ordering::SeqCst);
        self.parked.wake();
    }
}

struct ParkRegistration<'a>(&'a AtomicWaker);
impl Drop for ParkRegistration<'_> {
    fn drop(&mut self) {
        // Release the outer task on resumption or cancellation, including when
        // an executor waker escapes and outlives this run.
        self.0.take();
    }
}

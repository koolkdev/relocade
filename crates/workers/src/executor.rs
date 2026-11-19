//! Polling and parking for a worker's root future.

mod host;
pub use host::HostTurns;

use super::{WakeConnection, WakePeer, WakeTarget};
use async_channel::{Receiver, Sender};
use futures_timer::Delay;
use futures_util::future::{select, Either};
use host::HostSchedule;
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

/// Owns polling, cancellation and parking for a worker's root future.
/// Application and device futures are composed into one root future. Runnable
/// work is repolled here. Idle waits and requested host turns use the caller's executor.
pub struct Executor<R: WakeTarget> {
    connection: WakeConnection<R>,
    wake: Arc<ExecutorWake>,
    runnable: Receiver<()>,
    pub(crate) host: HostSchedule,
}

impl<R: WakeTarget> Executor<R> {
    /// Captures the signal binding for an additional worker's setup.
    pub fn peer(&self) -> WakePeer<R> {
        self.connection.peer()
    }

    pub fn new(connection: WakeConnection<R>) -> Self {
        let (notify, runnable) = async_channel::bounded(1);
        let wake = Arc::new(ExecutorWake {
            interrupt: connection.waker().clone(),
            notify,
            notified: AtomicBool::new(false),
        });
        let host = HostSchedule::new(connection.waker().clone());
        Self {
            connection,
            wake,
            runnable,
            host,
        }
    }

    /// Lets runnable work continue for `interval` before each pending delivery turn.
    /// The timer must progress independently of this executor and guest time.
    /// Without a timer, pending posted messages request a host turn immediately.
    /// Receivers bound with [`crate::Receiver::with_executor`] share one timer.
    pub fn with_host_timer<F: Future<Output = ()> + 'static>(
        mut self,
        interval: Duration,
        timer: impl Fn(Duration) -> F + 'static,
    ) -> Self {
        self.host.set_timer(interval, timer);
        self
    }

    /// Measures the host grace interval at execution checkpoints. While a host
    /// dependency is pending the executor requests a checkpoint on each turn.
    /// Synchronous work must observe the bound interrupt and return to Rust.
    pub fn with_cooperative_host_grace(mut self, interval: Duration) -> Self {
        self.host.cooperative_grace(interval);
        self
    }

    /// Registers work whose completion needs this worker's host event loop.
    pub fn host_turns(&self) -> HostTurns {
        self.host.pending.clone()
    }

    /// Runs the worker's root future until it completes or stop resolves.
    /// Stop is polled first, registering its waker before application code can
    /// enter synchronous guest execution. Cancellation returns None and drops
    /// the root future on its owner. Run one root at a time with this executor.
    pub async fn run_until<S: Future, F: Future>(&self, stop: S, future: F) -> Option<F::Output> {
        let mut stop = pin!(stop);
        let mut future = pin!(future);
        let waker = Waker::from(self.wake.clone());
        let mut grace = None;
        loop {
            self.wake.notified.store(false, Ordering::SeqCst);
            let mut context = Context::from_waker(&waker);
            if stop.as_mut().poll(&mut context).is_ready() {
                return None;
            }
            let result = future.as_mut().poll(&mut context);
            // A newly woken sibling may already have been polled. Preserve its
            // checkpoint through the next root poll, even across acknowledgement.
            self.connection.acknowledge();
            if self.wake.notified.load(Ordering::SeqCst) {
                self.wake.interrupt.wake_by_ref();
            }
            if let Poll::Ready(value) = result {
                return Some(value);
            }
            if self.runnable.try_recv().is_err() {
                // Parking itself lets the browser deliver pending callbacks.
                grace = None;
                self.runnable
                    .recv()
                    .await
                    .expect("executor owns its wake sender");
            }
            if self.host.turn_due(&mut grace, &mut context) {
                if let Either::Left(_) = select(stop.as_mut(), Delay::new(Duration::ZERO)).await {
                    return None;
                }
                // The browser may still owe a callback. Arm its next interval
                // before synchronous execution resumes. Stop is checked above.
                if self.host.turn_due(&mut grace, &mut context) {
                    self.wake.interrupt.wake_by_ref();
                }
            }
        }
    }

    /// Whether work has already requested another root poll.
    pub fn has_pending_poll(&self) -> bool {
        !self.runnable.is_empty()
    }

    /// Requests another poll of the root without interrupting the current turn.
    pub fn request_poll(&self) {
        let _ = self.wake.notify.try_send(());
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
    notify: Sender<()>,
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
        // One retained notification covers all wakes before the next poll.
        let _ = self.notify.try_send(());
    }
}

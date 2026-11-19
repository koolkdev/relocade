//! One dispatch loop for runnable work, cancellation and host delivery.

mod host;
pub use host::HostTurns;
mod queue;
mod root;

use super::{WakeConnection, WakePeer, WakeTarget};
use futures_util::task::AtomicWaker;
use host::HostSchedule;
use queue::ReadyQueue;
use std::{
    cell::RefCell,
    future::{poll_fn, Future},
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

/// Drives a worker's ready queue and interrupts synchronous execution on a wake.
/// Tasks and their futures stay on their owner. Their wakers can cross threads.
/// Run one session at a time on an executor.
pub struct Executor<R: WakeTarget> {
    connection: WakeConnection<R>,
    wake: Arc<WorkerWake>,
    tasks: RefCell<ReadyQueue>,
    pub(crate) host: HostSchedule,
}

impl<R: WakeTarget> Executor<R> {
    /// Captures the signal binding for an additional worker's setup.
    pub fn peer(&self) -> WakePeer<R> {
        self.connection.peer()
    }

    pub fn new(connection: WakeConnection<R>) -> Self {
        let wake = Arc::new(WorkerWake {
            interrupt: connection.waker().clone(),
            parked: AtomicWaker::new(),
            notified: AtomicBool::new(false),
        });
        let host = HostSchedule::new(Waker::from(wake.clone()));
        Self {
            connection,
            wake,
            tasks: RefCell::default(),
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

    /// Declares pending host operations on this worker, including posted objects
    /// and promises. Keep the handle with the operation that needs host delivery.
    pub fn host_turns(&self) -> HostTurns {
        self.host.pending.clone()
    }

    /// Creates a waker for one task in the current session. Wake it for its
    /// first poll. Use one waker per ID and clone it for additional waiters.
    pub fn task(&self, id: usize) -> Waker {
        self.tasks.borrow().task(id, self.wake.clone())
    }

    /// Whether a turn is waiting, including a re-poll requested by the active
    /// task. New wakes still request a synchronous execution checkpoint.
    pub fn has_ready(&self) -> bool {
        self.tasks.borrow().has_ready()
    }

    /// Polls one ready task per turn until a task completes the session or
    /// `stop` resolves. A pending task needs a wake to run again. Waking a
    /// task during its poll places it at the tail when the poll returns.
    /// Completion and cancellation discard queued work and retire these wakers.
    pub async fn drive<T>(
        &self,
        stop: impl Future,
        mut poll: impl FnMut(usize, &mut Context<'_>) -> Poll<T>,
    ) -> Option<T> {
        let _session = queue::Session(&self.tasks);
        self.dispatch(
            stop,
            || self.has_ready(),
            || {
                let task = self.tasks.borrow_mut().begin_turn();
                let Some((id, waker)) = task else {
                    return Poll::Pending;
                };
                let result = poll(id, &mut Context::from_waker(&waker));
                self.tasks.borrow_mut().end_turn();
                if self.has_ready() {
                    self.wake.notified.store(true, Ordering::SeqCst);
                }
                result
            },
        )
        .await
    }

    async fn dispatch<T>(
        &self,
        stop: impl Future,
        runnable: impl Fn() -> bool,
        mut poll: impl FnMut() -> Poll<T>,
    ) -> Option<T> {
        let mut stop = pin!(stop);
        let waker = Waker::from(self.wake.clone());
        let mut host = self.host.run();
        loop {
            self.wake.notified.store(false, Ordering::SeqCst);
            // Acknowledge at the stopped boundary *before* checking stop, host
            // demand or queued work. Wakes after that check can break this turn.
            self.connection.acknowledge();
            let mut cx = Context::from_waker(&waker);
            if stop.as_mut().poll(&mut cx).is_ready() {
                return None;
            }
            if runnable() && host.poll_ready(&mut cx).is_ready() {
                if let Poll::Ready(result) = poll() {
                    return Some(result);
                }
            }
            if self.wake.notified.load(Ordering::SeqCst) {
                continue;
            }
            if !runnable() {
                // Parking already gives JS a turn. Keep the operation's
                // registration, and arm grace again when Rust work resumes.
                host.idle();
            }
            let _registration = ParkRegistration(&self.wake.parked);
            poll_fn(|cx| {
                self.wake.parked.register(cx.waker());
                if self.wake.notified.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
    }
}

struct WorkerWake {
    interrupt: Waker,
    parked: AtomicWaker,
    notified: AtomicBool,
}

impl WorkerWake {
    fn schedule(&self) {
        self.notified.store(true, Ordering::SeqCst);
        self.parked.wake();
    }
}

impl Wake for WorkerWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.interrupt.wake_by_ref();
        self.schedule();
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

#[cfg(test)]
mod tests;

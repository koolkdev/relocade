//! Interrupts running work and retains notifications until the worker can poll.

use crate::{WakeConnection, WakePeer, WakeTarget};
use async_channel::{Receiver, Sender};
use futures_timer::Delay;
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
use web_time::Instant;

// A posted browser object can arrive after its Rust wake. Keep running ready
// work briefly before returning this worker to the browser's event loop.
const HOST_DELIVERY_GRACE: Duration = Duration::from_millis(1);

/// Connects a worker's polled futures, synchronous checkpoints and idle wait.
pub struct WorkerWake<R: WakeTarget> {
    connection: WakeConnection<R>,
    wake: Arc<Notify>,
    waker: Waker,
    runnable: Receiver<()>,
    host: HostTurns,
}

impl<R: WakeTarget> WorkerWake<R> {
    /// Binds notifications to the running worker's interrupt and idle wait.
    pub fn new(connection: WakeConnection<R>) -> Self {
        let (notify, runnable) = async_channel::bounded(1);
        let wake = Arc::new(Notify {
            interrupt: connection.waker().clone(),
            notify,
            notified: AtomicBool::new(false),
        });
        Self {
            host: HostTurns(Rc::new(Pending {
                count: Cell::new(0),
                interrupt: connection.waker().clone(),
            })),
            waker: Waker::from(wake.clone()),
            wake,
            runnable,
            connection,
        }
    }

    /// Wakes the root task and interrupts synchronous work on this worker.
    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    /// Captures the signal binding for an additional worker's setup.
    pub fn peer(&self) -> WakePeer<R> {
        self.connection.peer()
    }

    /// Registers operations that require this worker's host event loop.
    pub fn host_turns(&self) -> HostTurns {
        self.host.clone()
    }

    /// Drives the worker's root future. A runnable root requests another poll,
    /// while a blocked root registers its waker with the work it awaits.
    /// Compose cancellation before synchronous work so stop can interrupt it.
    pub async fn drive<F: Future>(&self, work: F) -> F::Output {
        let mut work = pin!(work);
        let mut host_deadline = None;
        loop {
            if let Poll::Ready(result) = self.poll(|cx| work.as_mut().poll(cx)) {
                return result;
            }
            if self.runnable.try_recv().is_err() {
                // Parking already gives the host an opportunity to deliver callbacks.
                host_deadline = None;
                self.wait().await;
            }
            if self.host.has_pending() {
                let deadline =
                    host_deadline.get_or_insert_with(|| Instant::now() + HOST_DELIVERY_GRACE);
                if Instant::now() >= *deadline {
                    Delay::new(Duration::ZERO).await;
                    host_deadline = None;
                }
                // Synchronous work must return to check the deadline even if
                // this was its last Rust wake before JavaScript delivers.
                self.request_checkpoint();
            } else {
                host_deadline = None;
            }
        }
    }

    /// Polls one turn with a waker that can interrupt synchronous work. An
    /// interrupt stays set through the whole turn so later siblings see it.
    pub fn poll<T>(&self, poll: impl FnOnce(&mut Context<'_>) -> T) -> T {
        self.wake.notified.store(false, Ordering::SeqCst);
        let result = poll(&mut Context::from_waker(&self.waker));
        self.connection.acknowledge();
        // A sibling can wake after its poll, including during acknowledgement.
        // Preserve its checkpoint for the next turn.
        if self.wake.notified.load(Ordering::SeqCst) {
            self.request_checkpoint();
        }
        result
    }

    /// Requests a checkpoint in synchronous work without scheduling a task.
    fn request_checkpoint(&self) {
        self.connection.waker().wake_by_ref();
    }

    /// Schedules another poll of the current future after a cooperative checkpoint.
    /// A nested task must go through its own waker to rejoin its parent's ready queue.
    pub fn repoll(&self, cx: &Context<'_>) {
        if self.waker.will_wake(cx.waker()) {
            let _ = self.wake.notify.try_send(());
        } else {
            cx.waker().wake_by_ref();
        }
    }

    /// Whether a notification is already queued for the next poll.
    pub fn has_pending_poll(&self) -> bool {
        !self.runnable.is_empty()
    }

    /// Consumes one retained notification, parking only if none is queued.
    pub async fn wait(&self) {
        self.runnable.recv().await.expect("worker owns its sender");
    }
}

struct Notify {
    interrupt: Waker,
    notify: Sender<()>,
    notified: AtomicBool,
}

impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notified.store(true, Ordering::SeqCst);
        self.interrupt.wake_by_ref();
        // A single retained notification covers all wakes until the next poll.
        let _ = self.notify.try_send(());
    }
}

/// Tracks operations whose completion requires an event-loop turn on this worker.
#[derive(Clone)]
pub struct HostTurns(Rc<Pending>);

impl HostTurns {
    /// Registers only after the operation's first pending poll. Completion and
    /// cancellation release its request, including while other waits are pending.
    pub async fn wait<F: Future>(&self, future: F) -> F::Output {
        let mut future = pin!(future);
        let mut registration = None;
        poll_fn(|cx| {
            let result = future.as_mut().poll(cx);
            if result.is_pending() && registration.is_none() {
                let pending = &*self.0;
                let count = pending.count.get();
                pending.count.set(count + 1);
                registration = Some(Registration(pending));
                if count == 0 {
                    pending.interrupt.wake_by_ref();
                }
            }
            result
        })
        .await
    }

    /// Whether any registered operation still needs host delivery.
    pub fn has_pending(&self) -> bool {
        self.0.count.get() != 0
    }
}

struct Pending {
    count: Cell<usize>,
    interrupt: Waker,
}

struct Registration<'a>(&'a Pending);
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.0.count.set(self.0.count.get() - 1);
    }
}

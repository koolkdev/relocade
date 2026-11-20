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
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};
use web_time::Instant;

// A posted browser object can arrive after its Rust wake. Keep running ready
// work briefly before returning this worker to the browser's event loop.
const HOST_DELIVERY_GRACE: Duration = Duration::from_millis(1);

const WORKER_READY: u8 = 1 << 7;

/// Coalesces readiness notifications shared by a worker and its application.
/// Application owners use bits 0 through 6. Passing bit 7 panics.
/// Recording a reason does not itself wake a task.
#[derive(Default)]
pub struct WakeReasons {
    bits: AtomicU8,
}

impl WakeReasons {
    /// Publish the selected reasons after making their associated work ready.
    pub fn record(&self, mask: u8) {
        self.bits.fetch_or(application_mask(mask), Ordering::SeqCst);
    }

    /// Acknowledge the selected reasons and return the bits that were pending.
    pub fn take(&self, mask: u8) -> u8 {
        // Unrelated turns and clean guest entries only read this shared byte.
        if self.pending(mask) {
            self.bits.fetch_and(!mask, Ordering::SeqCst) & mask
        } else {
            0
        }
    }

    /// Whether any of the selected reasons are pending.
    pub fn pending(&self, mask: u8) -> bool {
        self.bits.load(Ordering::SeqCst) & application_mask(mask) != 0
    }
}

fn application_mask(mask: u8) -> u8 {
    assert!(
        mask & WORKER_READY == 0,
        "wake reason bit 7 belongs to the worker"
    );
    mask
}

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
        Self::with_reasons(connection, Arc::default())
    }

    /// Shares the worker's readiness mask with application work on this connection.
    pub fn with_reasons(connection: WakeConnection<R>, reasons: Arc<WakeReasons>) -> Self {
        let (notify, runnable) = async_channel::bounded(1);
        let wake = Arc::new(Notify {
            interrupt: connection.waker().clone(),
            notify,
            reasons,
        });
        Self {
            host: HostTurns::new(connection.waker()),
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
        self.wake
            .reasons
            .bits
            .fetch_and(!WORKER_READY, Ordering::SeqCst);
        let result = poll(&mut Context::from_waker(&self.waker));
        self.connection.acknowledge();
        // A sibling can wake after its poll, including during acknowledgement.
        // Preserve its checkpoint for the next turn.
        if self.wake.reasons.bits.load(Ordering::SeqCst) & WORKER_READY != 0 {
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
    reasons: Arc<WakeReasons>,
}

impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.reasons.bits.fetch_or(WORKER_READY, Ordering::SeqCst);
        self.interrupt.wake_by_ref();
        // A single retained notification covers all wakes until the next poll.
        let _ = self.notify.try_send(());
    }
}

/// Tracks operations whose completion requires an event-loop turn on this worker.
/// Native workers and standalone consumers await operations directly.
#[derive(Clone, Default)]
pub struct HostTurns(Option<Rc<Pending>>);

impl HostTurns {
    fn new(interrupt: &Waker) -> Self {
        Self(cfg!(target_arch = "wasm32").then(|| {
            Rc::new(Pending {
                count: Cell::new(0),
                interrupt: interrupt.clone(),
            })
        }))
    }

    /// Registers only after the operation's first pending poll. Completion and
    /// cancellation release its request, including while other waits are pending.
    pub async fn wait<F: Future>(&self, future: F) -> F::Output {
        let Some(pending) = self.0.as_deref() else {
            return future.await;
        };
        let mut future = pin!(future);
        let mut registration = None;
        poll_fn(|cx| {
            let result = future.as_mut().poll(cx);
            if result.is_pending() && registration.is_none() {
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
        self.0
            .as_ref()
            .is_some_and(|pending| pending.count.get() != 0)
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

#[cfg(test)]
mod tests;

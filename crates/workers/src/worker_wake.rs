//! Interrupts running work and retains notifications until the worker can poll.

use crate::{WakeConnection, WakePeer, WakeTarget};
use async_channel::{Receiver, Sender};
use std::{
    cell::Cell,
    future::{poll_fn, Future},
    pin::pin,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Wake, Waker},
    time::Duration,
};

/// Connects a worker's polled futures, synchronous checkpoints and idle wait.
pub struct WorkerWake<R: WakeTarget> {
    connection: WakeConnection<R>,
    wake: Arc<Notify>,
    runnable: Receiver<()>,
    host: HostTurns,
}

impl<R: WakeTarget> WorkerWake<R> {
    /// Binds notifications to the running worker's interrupt and idle wait.
    pub fn new(connection: WakeConnection<R>) -> Self {
        let (notify, runnable) = async_channel::bounded(1);
        Self {
            host: HostTurns(Rc::new(Pending {
                count: Cell::new(0),
                interrupt: connection.waker().clone(),
            })),
            wake: Arc::new(Notify {
                interrupt: connection.waker().clone(),
                notify,
                notified: AtomicBool::new(false),
            }),
            runnable,
            connection,
        }
    }

    /// Captures the signal binding for an additional worker's setup.
    pub fn peer(&self) -> WakePeer<R> {
        self.connection.peer()
    }

    /// Registers operations that require this worker's host event loop.
    pub fn host_turns(&self) -> HostTurns {
        self.host.clone()
    }

    /// Polls one turn with a waker that can interrupt synchronous work. An
    /// interrupt stays set through the whole turn so later siblings see it.
    pub fn poll<T>(&self, poll: impl FnOnce(&mut Context<'_>) -> T) -> T {
        self.wake.notified.store(false, Ordering::SeqCst);
        let waker = Waker::from(self.wake.clone());
        let result = poll(&mut Context::from_waker(&waker));
        self.connection.acknowledge();
        // A sibling can wake after its poll, including during acknowledgement.
        // Preserve its checkpoint for the next turn.
        if self.wake.notified.load(Ordering::SeqCst) {
            self.request_checkpoint();
        }
        result
    }

    /// Requests a checkpoint in synchronous work without scheduling a task.
    pub fn request_checkpoint(&self) {
        self.connection.waker().wake_by_ref();
    }

    /// Schedules another poll without interrupting the current one.
    pub fn request_poll(&self) {
        let _ = self.wake.notify.try_send(());
    }

    /// Whether a notification is already queued for the next poll.
    pub fn has_pending_poll(&self) -> bool {
        !self.runnable.is_empty()
    }

    /// Consumes a retained notification, if one was queued during this turn.
    pub fn take_pending_poll(&self) -> bool {
        self.runnable.try_recv().is_ok()
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

/// Gives the worker's host event loop an opportunity to deliver callbacks.
pub async fn yield_to_host() {
    futures_timer::Delay::new(Duration::ZERO).await;
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

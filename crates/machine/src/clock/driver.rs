use crate::worker::{CancellableWorker, ThreadExt, WakePeer, WakeTarget, WorkerResource};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};
use web_time::Instant;

pub(super) struct Request {
    pub deadline: Instant,
    pub complete: oneshot::Sender<()>,
}

/// Starts the worker that owns pending timers and notifies their waiters.
pub struct TimerDriver {
    requests: mpsc::UnboundedReceiver<Request>,
}

impl TimerDriver {
    pub(super) fn new(requests: mpsc::UnboundedReceiver<Request>) -> Self {
        Self { requests }
    }

    /// Services waits until every timer handle and pending wait is released.
    /// Await through completion; use [`Self::spawn`] for explicit shutdown.
    pub async fn run(self) {
        self.spawn().join().await.expect("timer worker failed");
    }

    /// Starts a timer worker with explicit stop and join operations.
    pub fn spawn(self) -> CancellableWorker<()> {
        CancellableWorker::spawn(|thread, stopped| {
            thread.run_send((), move |()| async move { self.drive(stopped) })
        })
    }

    /// Binds direct execution notifications on the timer worker at startup.
    /// Browser timer delivery can then interrupt a busy CPU without a host relay.
    pub fn spawn_with_peer<R: WakeTarget + WorkerResource>(
        self,
        peer: WakePeer<R>,
    ) -> CancellableWorker<()> {
        CancellableWorker::spawn(|thread, stopped| {
            thread.run_resource(peer, move |peer| async move {
                let _binding = peer.bind();
                self.drive(stopped);
            })
        })
    }

    fn drive(mut self, mut stopped: oneshot::Receiver<()>) {
        let waker = Waker::from(Arc::new(TimerWake(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut timers: Vec<Request> = Vec::new();
        let mut closed = false;
        loop {
            if Pin::new(&mut stopped).poll(&mut context).is_ready() {
                return;
            }
            while !closed {
                match self.requests.poll_next_unpin(&mut context) {
                    Poll::Ready(Some(request)) => timers.push(request),
                    Poll::Ready(None) => closed = true,
                    Poll::Pending => break,
                }
            }
            let now = Instant::now();
            for timer in timers.extract_if(.., |timer| {
                timer.complete.poll_canceled(&mut context).is_ready() || timer.deadline <= now
            }) {
                let _ = timer.complete.send(());
            }
            if closed && timers.is_empty() {
                return;
            }
            // Request, cancellation and stop wakers all unpark this thread.
            // An unpark before parking retains a token, so no wake can be lost.
            // Our threaded Wasm std uses memory.atomic.wait32 for both waits.
            match timers.iter().map(|timer| timer.deadline).min() {
                Some(deadline) => {
                    thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
                }
                None => thread::park(),
            }
        }
    }
}

struct TimerWake(thread::Thread);

impl Wake for TimerWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

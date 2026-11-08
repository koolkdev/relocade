use crate::worker::{CancellableWorker, ThreadExt, WakePeer, WakeTarget, WorkerResource};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use web_time::Instant;

mod wait;

pub(super) struct Request {
    pub deadline: Instant,
    pub complete: oneshot::Sender<()>,
}

/// Starts the worker that owns pending timers and notifies their waiters.
pub struct HostTimerDriver {
    requests: mpsc::UnboundedReceiver<Request>,
}

impl HostTimerDriver {
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
            thread.run_send((), move |()| self.drive(stopped))
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
                self.drive(stopped).await;
            })
        })
    }

    async fn drive(mut self, mut stopped: oneshot::Receiver<()>) {
        let mut wait = wait::Wait::new();
        let waker = wait.waker().clone();
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
            // Requests, cancellation and shutdown share a retained notification,
            // including notifications that arrive between polling and waiting.
            wait.until(timers.iter().map(|timer| timer.deadline).min())
                .await;
        }
    }
}

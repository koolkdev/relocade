//! Ordered resource delivery to a receiver bound to its execution owner.

use crate::{ResourceTransfer, Worker, WorkerResource};
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::{
    fmt,
    task::{ready, Context, Poll},
};

#[cfg_attr(not(target_arch = "wasm32"), path = "channel/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "channel/web.rs")]
mod platform;

type Rebuild<R> = Box<dyn FnOnce(<R as WorkerResource>::Posted) -> R + Send>;

/// Failure to send or receive a worker resource.
#[derive(Clone, Debug)]
pub enum ResourceError {
    Closed,
    Transport(String),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("worker resource channel closed"),
            Self::Transport(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for ResourceError {}

impl From<web_thread::Error> for ResourceError {
    fn from(error: web_thread::Error) -> Self {
        Self::Transport(error.to_string())
    }
}

/// Sends complete objects to one receiver. This handle can be transferred with
/// WorkerResource; its reconstruction state stays in ordinary Rust memory.
pub struct Sender<R: WorkerResource> {
    posted: platform::Sender<R::Posted>,
    rebuild: mpsc::UnboundedSender<Rebuild<R>>,
}

impl<R: WorkerResource> WorkerResource for Sender<R> {
    type Posted = platform::Sender<R::Posted>;

    fn prepare(self) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send> {
        ResourceTransfer::new(self.posted, move |posted| Self {
            posted,
            rebuild: self.rebuild,
        })
    }
}

/// Receives complete objects on the owner chosen when the channel was created.
/// This sendable handle can be captured by a worker task; receive operations
/// must run on its destination. It does not keep that worker alive.
/// Cancelling a receive preserves both the queued payload and Rust state.
pub struct Receiver<R: WorkerResource> {
    posted: platform::Receiver<R::Posted>,
    rebuild: mpsc::UnboundedReceiver<Rebuild<R>>,
    pending: Option<Rebuild<R>>,
}

/// Creates a channel that receives on the calling thread. Use [`Worker::channel`]
/// to receive on another worker. Browser receipt needs an event-loop turn;
/// this transport does not choose when an execution worker yields to its host.
pub fn channel<R: WorkerResource>() -> Result<(Sender<R>, Receiver<R>), ResourceError> {
    let (sender, receiver) = platform::channel()?;
    Ok(pair(sender, receiver.bind()))
}

pub(crate) async fn on_worker<R: WorkerResource>(
    worker: &Worker,
) -> Result<(Sender<R>, Receiver<R>), ResourceError> {
    let (sender, endpoint) = platform::channel()?;
    let receiver = worker
        .thread
        .run_send(endpoint, |endpoint| async move { endpoint.bind() })
        .await?;
    Ok(pair(sender, receiver))
}

fn pair<R: WorkerResource>(
    sender: platform::Sender<R::Posted>,
    receiver: platform::Receiver<R::Posted>,
) -> (Sender<R>, Receiver<R>) {
    let (rebuild, incoming) = mpsc::unbounded();
    (
        Sender {
            posted: sender,
            rebuild,
        },
        Receiver {
            posted: receiver,
            rebuild: incoming,
            pending: None,
        },
    )
}

impl<R: WorkerResource> Sender<R> {
    /// Posts an object without waiting for receipt. A successful send publishes
    /// its Rust reconstruction state only after posting the engine payload.
    /// Exclusive access keeps these two deliveries in the same order.
    pub fn send(&mut self, resource: R) -> Result<(), ResourceError> {
        if self.rebuild.is_closed() {
            return Err(ResourceError::Closed);
        }
        let ResourceTransfer { posted, rebuild } = resource.prepare();
        self.posted.send(posted)?;
        self.rebuild
            .unbounded_send(Box::new(rebuild))
            .map_err(|_| ResourceError::Closed)
    }
}

impl<R: WorkerResource> Receiver<R> {
    /// Receives the next object. Normal sender closure follows all accepted
    /// objects, including payloads still awaiting browser delivery. Abrupt browser
    /// worker termination does not guarantee Rust cleanup or a close notification.
    pub async fn recv(&mut self) -> Option<Result<R, ResourceError>> {
        std::future::poll_fn(|cx| self.poll_recv(cx)).await
    }

    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<R, ResourceError>>> {
        self.posted.assert_owner();
        if self.pending.is_none() {
            let Some(rebuild) = ready!(self.rebuild.poll_next_unpin(cx)) else {
                return Poll::Ready(None);
            };
            self.pending = Some(rebuild);
        }
        let posted = ready!(self.posted.poll_recv(cx));
        let rebuild = self.pending.take().expect("pending reconstruction");
        Poll::Ready(Some(posted.map(rebuild)))
    }
}

#[cfg(test)]
mod tests;

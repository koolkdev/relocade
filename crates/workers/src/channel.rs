//! Repeated delivery using the same resource definition as worker task transfers.

use crate::{ResourceTransfer, WorkerResource};
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

/// Sends complete objects to one receiver. This handle can be transferred with
/// WorkerResource; its reconstruction state stays in ordinary Rust memory.
pub struct Sender<R: WorkerResource> {
    posted: platform::Sender<R::Posted>,
    rebuild: mpsc::UnboundedSender<Rebuild<R>>,
}

/// A receiver that can be transferred before it is bound to its final owner.
pub struct ReceiverEndpoint<R: WorkerResource> {
    posted: platform::ReceiverEndpoint<R::Posted>,
    rebuild: mpsc::UnboundedReceiver<Rebuild<R>>,
}

/// Receives complete objects on the owner where its endpoint was bound.
/// Cancelling a receive preserves both the queued payload and Rust state.
pub struct Receiver<R: WorkerResource> {
    posted: platform::Receiver<R::Posted>,
    rebuild: mpsc::UnboundedReceiver<Rebuild<R>>,
    pending: Option<Rebuild<R>>,
}

/// Creates a resource sender and a transferable receiver endpoint. Bind the
/// receiver on its final owner. Browser receipt needs an event-loop turn;
/// this transport does not choose when an execution worker yields to its host.
pub fn channel<R: WorkerResource>() -> Result<(Sender<R>, ReceiverEndpoint<R>), ResourceError> {
    let (sender, receiver) = platform::channel()?;
    let (rebuild, incoming) = mpsc::unbounded();
    Ok((
        Sender {
            posted: sender,
            rebuild,
        },
        ReceiverEndpoint {
            posted: receiver,
            rebuild: incoming,
        },
    ))
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

impl<R: WorkerResource> ReceiverEndpoint<R> {
    pub fn bind(self) -> Receiver<R> {
        Receiver {
            posted: self.posted.bind(),
            rebuild: self.rebuild,
            pending: None,
        }
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

impl<R: WorkerResource> WorkerResource for Sender<R> {
    type Posted = platform::Sender<R::Posted>;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static> {
        ResourceTransfer::new(self.posted, move |posted| Self {
            posted,
            rebuild: self.rebuild,
        })
    }
}

impl<R: WorkerResource> WorkerResource for ReceiverEndpoint<R> {
    type Posted = platform::ReceiverEndpoint<R::Posted>;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static> {
        ResourceTransfer::new(self.posted, move |posted| Self {
            posted,
            rebuild: self.rebuild,
        })
    }
}

#[cfg(test)]
mod tests;

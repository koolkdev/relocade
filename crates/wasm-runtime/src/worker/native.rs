use super::Post;
use crate::{Error, Result};
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::task::{Context, Poll};

/// A resource channel endpoint that can move to its final worker before binding.
pub struct Endpoint<T: Post>(Port<T>);

impl<T: Post> Endpoint<T> {
    /// Binds this endpoint to its receiving owner.
    pub fn bind(self) -> Port<T> {
        self.0
    }
}

/// A bound port for sending and receiving engine resources.
pub struct Port<T: Post> {
    outgoing: mpsc::UnboundedSender<T>,
    incoming: mpsc::UnboundedReceiver<T>,
}

/// Creates two transferable endpoints. Bind each on its final owner.
pub fn channel<T: Post>() -> Result<(Endpoint<T>, Endpoint<T>)> {
    let (a_tx, a_rx) = mpsc::unbounded();
    let (b_tx, b_rx) = mpsc::unbounded();
    Ok((
        Endpoint(Port {
            outgoing: a_tx,
            incoming: b_rx,
        }),
        Endpoint(Port {
            outgoing: b_tx,
            incoming: a_rx,
        }),
    ))
}

impl<T: Post> Port<T> {
    /// Queues a resource for the peer without waiting for it to receive.
    pub fn send(&self, value: T) -> Result<()> {
        self.outgoing
            .unbounded_send(value)
            .map_err(|error| Error::new("send worker resource", error))
    }

    /// Polls delivery and registers the caller for the next value or normal close.
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<T>>> {
        self.incoming.poll_next_unpin(cx).map(|value| value.map(Ok))
    }
}

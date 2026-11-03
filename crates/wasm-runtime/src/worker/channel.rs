//! Persistent resource transport with Rust reconstruction captures.

use super::{ResourceTransfer, WorkerResource};
use crate::{Error, Result};
use futures_channel::mpsc;
use futures_util::StreamExt;
use std::{
    future::poll_fn,
    task::{ready, Context, Poll},
};

#[cfg_attr(not(target_arch = "wasm32"), path = "channel/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "channel/web.rs")]
mod transport;

type Rebuild<R> = Box<dyn FnOnce(<R as WorkerResource>::Posted) -> R + Send>;

/// One end of a resource channel, transferable before it is bound to an owner.
pub struct Endpoint<R: WorkerResource> {
    posted: transport::Endpoint<R::Posted>,
    outgoing: mpsc::UnboundedSender<Rebuild<R>>,
    incoming: mpsc::UnboundedReceiver<Rebuild<R>>,
}

/// A bound resource channel. Rust captures and posted resources are reconstructed
/// together on receipt. Browser delivery needs an event-loop turn on this owner.
pub struct Port<R: WorkerResource> {
    posted: transport::Port<R::Posted>,
    outgoing: mpsc::UnboundedSender<Rebuild<R>>,
    incoming: mpsc::UnboundedReceiver<Rebuild<R>>,
    rebuild: Option<Rebuild<R>>,
}

/// Creates a pair of endpoints. Transfer an endpoint with `run_resource`, then
/// bind it on its receiving worker. Sending never waits for receipt.
pub fn channel<R: WorkerResource>() -> Result<(Endpoint<R>, Endpoint<R>)> {
    let (a, b) = transport::channel()?;
    let (a_tx, a_rx) = mpsc::unbounded();
    let (b_tx, b_rx) = mpsc::unbounded();
    Ok((
        Endpoint {
            posted: a,
            outgoing: a_tx,
            incoming: b_rx,
        },
        Endpoint {
            posted: b,
            outgoing: b_tx,
            incoming: a_rx,
        },
    ))
}

impl<R: WorkerResource> WorkerResource for Endpoint<R> {
    type Posted = transport::Endpoint<R::Posted>;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static> {
        let Self {
            posted,
            outgoing,
            incoming,
        } = self;
        ResourceTransfer::new(posted, move |posted| Self {
            posted,
            outgoing,
            incoming,
        })
    }
}

impl<R: WorkerResource> Endpoint<R> {
    /// Installs the receiving inbox on the current owner. Browser ports cannot
    /// be posted to another worker after binding.
    pub fn bind(self) -> Port<R> {
        Port {
            posted: self.posted.bind(),
            outgoing: self.outgoing,
            incoming: self.incoming,
            rebuild: None,
        }
    }
}

impl<R: WorkerResource> Port<R> {
    /// Queues one resource. Successful sending does not acknowledge its receipt.
    pub fn send(&mut self, value: R) -> Result<()> {
        let ResourceTransfer { posted, rebuild } = value.prepare();
        self.posted.send(posted)?;
        self.outgoing
            .unbounded_send(Box::new(rebuild))
            .map_err(|_| Error::new("send worker resource", "receiver closed"))
    }

    /// Receives the next resource or orderly closure. Cancelling this wait leaves
    /// partially delivered resources in the port for the next receiver.
    pub async fn recv(&mut self) -> Option<Result<R>> {
        poll_fn(|cx| self.poll_recv(cx)).await
    }

    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<R>>> {
        if self.rebuild.is_none() {
            let Some(rebuild) = ready!(self.incoming.poll_next_unpin(cx)) else {
                return Poll::Ready(None);
            };
            self.rebuild = Some(rebuild);
        }
        let posted = ready!(self.posted.poll_recv(cx)).unwrap_or_else(|| {
            Err(Error::new(
                "receive worker resource",
                "missing posted resource",
            ))
        });
        Poll::Ready(Some(posted.map(self.rebuild.take().unwrap())))
    }
}

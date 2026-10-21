use crate::{Error, Message, Result};
use futures_channel::oneshot;
use std::{
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

#[cfg(not(target_arch = "wasm32"))]
use crate::native::requests::{Incoming, Response, Transport};
#[cfg(target_arch = "wasm32")]
use crate::web::requests::{Incoming, Response, Transport};

/// A sequential request worker with bounded outstanding work.
///
/// Accepted requests start without polling their result futures. Dropping a
/// result discards that reply; it does not cancel work or free capacity early.
/// Shutdown and owner drop drain accepted requests. Results and lifecycle
/// futures can outlive this owner.
pub struct RequestWorker<C, R> {
    transport: Transport<C, R>,
    outstanding: Arc<AtomicUsize>,
    limit: usize,
    open: bool,
}

pub(crate) struct Reply<R> {
    sender: oneshot::Sender<Result<R>>,
    permit: Permit,
}

struct Permit(Arc<AtomicUsize>);

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

impl<R> Reply<R> {
    pub(crate) fn complete(self, result: Result<R>) {
        let Self { sender, permit } = self;
        // Release capacity before waking the caller, including discarded replies.
        drop(permit);
        let _ = sender.send(result);
    }
}

impl<C: Message, R: Message> RequestWorker<C, R> {
    pub(crate) fn new(transport: Transport<C, R>, limit: usize) -> Self {
        assert!(limit > 0, "a request worker needs positive capacity");
        Self {
            transport,
            outstanding: Arc::new(AtomicUsize::new(0)),
            limit,
            open: true,
        }
    }

    /// Returns None when the outstanding-request limit is full. Capacity is
    /// released when a reply arrives, independently of polling its future.
    pub fn submit(
        &mut self,
        request: C,
    ) -> Result<Option<impl Future<Output = Result<R>> + 'static>> {
        if !self.open {
            return Err(Error::Closed);
        }
        if self.outstanding.load(Ordering::Relaxed) >= self.limit {
            return Ok(None);
        }
        self.outstanding.fetch_add(1, Ordering::Relaxed);
        let (sender, completion) = oneshot::channel();
        let reply = Reply {
            sender,
            permit: Permit(Arc::clone(&self.outstanding)),
        };
        self.transport.send(request, reply)?;
        let stopped = self.transport.worker.stopped();
        Ok(Some(async move {
            match completion.await {
                Ok(result) => result,
                Err(_) => {
                    stopped.await?;
                    Err(Error::Closed)
                }
            }
        }))
    }

    pub fn ready(&self) -> impl Future<Output = Result<()>> + 'static {
        self.transport.worker.ready()
    }

    /// Closes submission, allowing accepted requests to finish.
    pub fn shutdown(&mut self) {
        self.open = false;
        self.transport.worker.shutdown();
    }

    pub fn stopped(&self) -> impl Future<Output = Result<()>> + 'static {
        self.transport.worker.stopped()
    }
}

/// The typed worker-side endpoint. Initialize local state before calling
/// `serve`; readiness is reported when the request handler is ready to run.
pub struct RequestEndpoint<C, R> {
    pub(crate) incoming: Incoming<C, R>,
}

pub(crate) struct Request<C, R> {
    pub(crate) input: C,
    pub(crate) response: Response<R>,
}

impl<C: Message, R: Message> RequestEndpoint<C, R> {
    /// Processes accepted requests in order, sending one result for each.
    /// Handler errors fail only their request. Input closure drains queued
    /// work before returning; transport failure terminates the entry.
    pub async fn serve(mut self, mut handle: impl AsyncFnMut(C) -> Result<R>) -> Result<()> {
        self.incoming.ready();
        while let Some(request) = self.incoming.recv().await? {
            request.response.send(handle(request.input).await)?;
        }
        Ok(())
    }
}

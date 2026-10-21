use super::{Endpoint, Worker};
use crate::{
    requests::{Reply, Request},
    Message, RequestEndpoint, RequestWorker, Result,
};
use std::future::Future;

pub(crate) struct Transport<C, R> {
    pub(crate) worker: Worker<Request<C, R>>,
}

pub(crate) struct Incoming<C, R>(Endpoint<Request<C, R>, ()>);
pub(crate) struct Response<R>(Reply<R>);

impl<C: Message, R: Message> RequestWorker<C, R> {
    /// Starts a typed entry on a dedicated thread. Initialize worker-local state
    /// there and pass its handler to `endpoint.serve`. The entry closure crosses
    /// threads; its future and the state created inside it need not be Send.
    pub fn spawn<Fut: Future<Output = Result<()>>>(
        name: &str,
        limit: usize,
        entry: impl FnOnce(RequestEndpoint<C, R>) -> Fut + Send + 'static,
    ) -> Result<Self> {
        let (worker, _) = Worker::spawn(name, move |endpoint| {
            entry(RequestEndpoint {
                incoming: Incoming(endpoint),
            })
        })?;
        Ok(Self::new(Transport { worker }, limit))
    }
}

impl<C, R> Transport<C, R> {
    pub(crate) fn send(&self, input: C, reply: Reply<R>) -> Result<()> {
        self.worker.send(Request {
            input,
            response: Response(reply),
        })
    }
}

impl<C, R> Incoming<C, R> {
    pub(crate) fn ready(&self) {
        self.0.ready();
    }

    pub(crate) async fn recv(&mut self) -> Result<Option<Request<C, R>>> {
        Ok(self.0.recv().await)
    }
}

impl<R> Response<R> {
    pub(crate) fn send(self, result: Result<R>) -> Result<()> {
        self.0.complete(result);
        Ok(())
    }
}

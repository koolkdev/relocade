use super::{worker_error, BlockRequest, CompiledCode, Result};
use crate::worker::{self, Receiver, ReceiverEndpoint, Sender, WorkerResource};
use futures_channel::mpsc;
use std::future::poll_fn;

/// A connection's local submission and receipt state. Await `compile` directly;
/// no companion task needs to be driven. A cancelled call leaves its response
/// owned here, and the next call drains it before submitting another request.
pub struct CompilerClient {
    requests: mpsc::Sender<Vec<BlockRequest>>,
    results: Receiver<Result<CompiledCode>>,
    in_flight: bool,
}

/// The compiler end of a connection. Transfer it to the compiler's host and
/// attach it with `CompilerWorker::connect`. Its client stays on execution.
#[derive(WorkerResource)]
pub struct CompilerConnection {
    pub(super) incoming: mpsc::Receiver<Vec<BlockRequest>>,
    #[worker(resource)]
    pub(super) results: Sender<Result<CompiledCode>>,
}

impl CompilerClient {
    /// Creates a connection on an ordinary async owner.
    pub fn channel() -> Result<(Self, CompilerConnection)> {
        channel(ReceiverEndpoint::bind)
    }

    /// Submits ranges and receives their complete response. Keep the requested
    /// bytes and mappings stable through accepted work, including an abandoned
    /// call. Dropping this client closes submission; the worker drains it.
    pub async fn compile(&mut self, blocks: Vec<BlockRequest>) -> Result<CompiledCode> {
        if self.in_flight {
            // A prior caller abandoned its response, including any compile error.
            // A transport failure still ends the connection.
            let _ = self.receive().await?;
        }
        poll_fn(|cx| self.requests.poll_ready(cx))
            .await
            .map_err(worker_error)?;
        self.requests.start_send(blocks).map_err(worker_error)?;
        self.in_flight = true;
        self.receive().await?
    }

    async fn receive(&mut self) -> Result<Result<CompiledCode>> {
        let result = self
            .results
            .recv()
            .await
            .ok_or_else(|| worker_error("compiler connection closed"))?;
        self.in_flight = false;
        result.map_err(worker_error)
    }
}

pub(crate) fn channel(
    bind: impl FnOnce(ReceiverEndpoint<Result<CompiledCode>>) -> Receiver<Result<CompiledCode>>,
) -> Result<(CompilerClient, CompilerConnection)> {
    let (results, input) = worker::channel().map_err(worker_error)?;
    let (requests, incoming) = mpsc::channel(8);
    Ok((
        CompilerClient {
            requests,
            results: bind(input),
            in_flight: false,
        },
        CompilerConnection { incoming, results },
    ))
}

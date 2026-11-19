use super::{
    connection::{Compilation, Request},
    BlockRequest, CompiledCode, FunctionRequest, Reply,
};
use crate::{
    worker::{ChannelError, ReceiverEndpoint, Sender, Transfer, WakePeer, WorkerWake},
    ExecutionInterrupt, Result,
};
use futures_channel::mpsc;
use futures_util::{
    stream::{self, LocalBoxStream},
    StreamExt,
};
use relocade_x86::ExecutionTarget;

/// One CPU's compilation requests and ordered results.
/// Connects directly to the shared service. Dropping it closes the request stream.
pub struct CompilerClient {
    requests: mpsc::UnboundedSender<Request>,
    results: LocalBoxStream<'static, Result<CompiledCode>>,
}

#[derive(Transfer)]
pub struct Endpoint {
    pub(super) requests: mpsc::UnboundedSender<Request>,
    pub(super) results: ReceiverEndpoint<Reply>,
    pub(super) peer: Sender<WakePeer<ExecutionInterrupt>>,
}

impl Endpoint {
    pub(crate) fn connect(
        mut self,
        worker: &WorkerWake<ExecutionInterrupt>,
    ) -> Result<CompilerClient> {
        self.peer.send(worker.peer())?;
        let results = self.results.bind().with_worker(worker);
        // Retain the receive future between CPU polls so its pending browser
        // delivery stays registered with the worker.
        let results = stream::unfold(results, async |mut results| {
            let result = match results.recv().await? {
                Ok(Reply(result)) => result,
                Err(error) => Err(error.into()),
            };
            Some((result, results))
        })
        .fuse()
        .boxed_local();
        Ok(CompilerClient {
            requests: self.requests,
            results,
        })
    }
}

impl CompilerClient {
    /// Queues compilation for this target using the machine's shared guest memory.
    /// The stopped CPU validates the emitted instruction bytes at installation.
    pub fn request(&self, target: ExecutionTarget, blocks: Vec<BlockRequest>) -> Result<()> {
        self.submit(target, Compilation::Blocks(blocks))
    }

    /// Queues guest functions for compilation into one module. Each requested
    /// entry is a root for block discovery. Discovery follows branches and call
    /// continuations, and shares blocks across roots. Request a callee by its
    /// entry to include it in the batch.
    /// Discovered blocks must have no interior host hooks or other registered
    /// execution boundaries.
    /// Rejected roots leave earlier discoveries intact. If no root yields blocks,
    /// the result reports the first discovery error, or is empty for unreadable roots.
    /// Receive and install the result in the same way as an explicit block request.
    pub fn request_functions(
        &self,
        target: ExecutionTarget,
        functions: Vec<FunctionRequest>,
    ) -> Result<()> {
        self.submit(target, Compilation::Functions(functions))
    }

    fn submit(&self, target: ExecutionTarget, compilation: Compilation) -> Result<()> {
        self.requests
            .unbounded_send(Request {
                target,
                compilation,
            })
            .map_err(|_| ChannelError::Closed.into())
    }

    /// Receives the next result in request order. Installation is explicit.
    /// Dropping this wait leaves that result available for the next receive.
    /// The client retains any unfinished receive and its host-delivery registration.
    /// Returns `None` once the compiler has finished and all replies are consumed.
    /// Poll within the Runner session that supplied this connection.
    pub async fn recv(&mut self) -> Option<Result<CompiledCode>> {
        self.results.next().await
    }
}

impl Drop for CompilerClient {
    fn drop(&mut self) {
        self.requests.close_channel();
    }
}

use super::{
    connection::{Compilation, Request},
    BlockRequest, CompiledCode, FunctionRequest, Reply,
};
use crate::{
    worker::{ChannelError, Executor, Receiver, ReceiverEndpoint, Sender, Transfer, WakePeer},
    ExecutionInterrupt, Result,
};
use futures_channel::mpsc;
use relocade_x86::ExecutionTarget;

/// One CPU's compilation requests and ordered results.
/// Connects directly to the shared service. Dropping it closes the request stream.
pub struct CompilerClient {
    requests: mpsc::UnboundedSender<Request>,
    results: Receiver<Reply>,
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
        executor: &Executor<ExecutionInterrupt>,
    ) -> Result<CompilerClient> {
        self.peer.send(executor.peer())?;
        Ok(CompilerClient {
            requests: self.requests,
            results: self.results.bind().with_executor(executor),
        })
    }
}

impl CompilerClient {
    /// Queues compilation for this target using the machine's shared guest memory.
    /// Keep code and mappings stable until the result arrives or the machine shuts down.
    pub fn request(&self, target: ExecutionTarget, blocks: Vec<BlockRequest>) -> Result<()> {
        self.submit(target, Compilation::Blocks(blocks))
    }

    /// Queues bounded discovery from one clean real-mode call target. Callees are
    /// not followed. Keep code, mappings and the entry's CS addressing stable;
    /// candidates must have no interior host hooks or registered execution boundaries.
    /// Results use the same ordered stream and explicit installation as block requests.
    pub fn request_function(
        &self,
        target: ExecutionTarget,
        function: FunctionRequest,
    ) -> Result<()> {
        self.submit(target, Compilation::Function(function))
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
    /// Returns `None` once the compiler has finished and all replies are consumed.
    /// Poll within the Runner session that supplied this connection.
    pub async fn recv(&mut self) -> Option<Result<CompiledCode>> {
        self.results.recv().await.map(|reply| match reply {
            Ok(Reply(result)) => result,
            Err(error) => Err(error.into()),
        })
    }
}

impl Drop for CompilerClient {
    fn drop(&mut self) {
        self.requests.close_channel();
    }
}

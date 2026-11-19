use super::{
    connection::{Compilation, Request},
    BlockRequest, CompiledCode, FunctionRequest, Reply,
};
use crate::{
    worker::{ChannelError, ReceiverEndpoint, Sender, Transfer, WakePeer, WorkerWake},
    ExecutionInterrupt, Result,
};
use futures_channel::mpsc;
use futures_util::{future::LocalBoxFuture, StreamExt};
use relocade_x86::ExecutionTarget;
use std::future::poll_fn;

/// One CPU's compilation requests and ordered results.
/// Connects directly to the shared service. Dropping it closes the request stream.
pub struct CompilerClient {
    requests: mpsc::UnboundedSender<Request>,
    results: mpsc::Receiver<Result<CompiledCode>>,
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
    ) -> Result<(CompilerClient, LocalBoxFuture<'static, ()>)> {
        self.peer.send(worker.peer())?;
        let mut posted = self.results.bind().with_worker(worker);
        // A single sender owns one slot in a zero-capacity futures channel.
        let (mut ready, results) = mpsc::channel(0);
        let receive = async move {
            // Finish transport on its own task even if the foreground stops
            // while a JS object is in flight. Waiting for client capacity
            // requests no host turns.
            while poll_fn(|cx| ready.poll_ready(cx)).await.is_ok() {
                let Some(reply) = posted.recv().await else {
                    break;
                };
                let result = match reply {
                    Ok(Reply(result)) => result,
                    Err(error) => Err(error.into()),
                };
                if ready.try_send(result).is_err() {
                    break;
                }
            }
        };
        Ok((
            CompilerClient {
                requests: self.requests,
                results,
            },
            Box::pin(receive),
        ))
    }
}

impl CompilerClient {
    /// Queues compilation for this target using the machine's shared guest memory.
    /// Keep code and mappings stable until the result arrives or the machine shuts down.
    pub fn request(&self, target: ExecutionTarget, blocks: Vec<BlockRequest>) -> Result<()> {
        self.submit(target, Compilation::Blocks(blocks))
    }

    /// Queues guest functions for compilation into one module. Each requested
    /// entry is a root for block discovery. Discovery follows branches and call
    /// continuations, and shares blocks across roots. Request a callee by its
    /// entry to include it in the batch.
    /// Keep code, mappings and CS addressing stable. Discovered blocks must have
    /// no interior host hooks or other registered execution boundaries.
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
    /// The session finishes posted-object delivery independently of this wait.
    /// Returns `None` once the compiler has finished and all replies are consumed.
    /// Poll within the Runner session that supplied this connection.
    pub async fn recv(&mut self) -> Option<Result<CompiledCode>> {
        self.results.next().await
    }

    /// Takes a delivered result at an eligible CPU boundary.
    pub(crate) fn try_recv(
        &mut self,
    ) -> std::result::Result<Result<CompiledCode>, mpsc::TryRecvError> {
        self.results.try_recv()
    }
}

impl Drop for CompilerClient {
    fn drop(&mut self) {
        self.requests.close_channel();
    }
}

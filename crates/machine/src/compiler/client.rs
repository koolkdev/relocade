use super::{
    connection::{Compilation, Request},
    BlockRequest, CompiledCode, FunctionRequest, Reply,
};
use crate::{
    worker::{ChannelError, Receiver, ReceiverEndpoint, Sender, Transfer, WakePeer, WorkerWake},
    ExecutionInterrupt, Result,
};
use futures_channel::mpsc;
use futures_util::{future::LocalBoxFuture, StreamExt};
use relocade_x86::CompilerConfig;
use std::{
    future::poll_fn,
    task::{Context, Poll},
};

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
        let posted = self.results.bind().with_worker(worker);
        // A single sender owns one slot in a zero-capacity futures channel.
        let (ready, results) = mpsc::channel(0);
        Ok((
            CompilerClient {
                requests: self.requests,
                results,
            },
            Box::pin(forward_replies(posted, ready)),
        ))
    }
}

// The session owns transport so cancelling a CPU run cannot strand a posted module.
async fn forward_replies(
    mut posted: Receiver<Reply>,
    mut ready: mpsc::Sender<Result<CompiledCode>>,
) {
    // Wait for local capacity before requesting a browser turn for the next reply.
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
}

impl CompilerClient {
    /// Queues compilation with the supplied settings and the machine's shared guest memory.
    /// The stopped CPU validates the emitted instruction bytes at installation.
    pub fn request(&self, config: CompilerConfig, blocks: Vec<BlockRequest>) -> Result<()> {
        self.submit(config, Compilation::Blocks(blocks))
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
        config: CompilerConfig,
        functions: Vec<FunctionRequest>,
    ) -> Result<()> {
        self.submit(config, Compilation::Functions(functions))
    }

    fn submit(&self, config: CompilerConfig, compilation: Compilation) -> Result<()> {
        self.requests
            .unbounded_send(Request {
                config,
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
        poll_fn(|cx| self.poll_reply(cx)).await
    }

    pub(crate) fn poll_reply(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<CompiledCode>>> {
        self.results.poll_next_unpin(cx)
    }
}

impl Drop for CompilerClient {
    fn drop(&mut self) {
        self.requests.close_channel();
    }
}

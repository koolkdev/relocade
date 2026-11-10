use super::{service::Request, BlockRequest, CompiledCode, Reply};
use crate::{
    worker::{ChannelError, Receiver, ReceiverEndpoint, Sender, Transfer, WakePeer},
    ExecutionInterrupt, Result,
};
use futures_channel::mpsc;
use relocade_x86::ExecutionTarget;

/// One CPU's compilation requests and ordered results.
/// The machine owns the compiler; dropping this connection closes its request stream.
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
    pub(crate) fn connect(mut self, peer: WakePeer<ExecutionInterrupt>) -> Result<CompilerClient> {
        self.peer.send(peer)?;
        Ok(CompilerClient {
            requests: self.requests,
            results: self.results.bind(),
        })
    }
}

impl CompilerClient {
    /// Queues compilation for this target using the machine's shared guest memory.
    /// Keep code and mappings stable until the result arrives or the machine shuts down.
    pub fn request(&self, target: ExecutionTarget, blocks: Vec<BlockRequest>) -> Result<()> {
        self.requests
            .unbounded_send(Request { target, blocks })
            .map_err(|_| ChannelError::Closed.into())
    }

    /// Receives the next result in request order. Installation is explicit.
    /// Dropping this wait leaves that result available for the next receive.
    /// Returns `None` once the compiler has finished and all replies are consumed.
    pub async fn recv(&mut self) -> Option<Result<CompiledCode>> {
        self.results.recv().await.map(|reply| match reply {
            Ok(Reply(result)) => result,
            Err(error) => Err(error.into()),
        })
    }
}

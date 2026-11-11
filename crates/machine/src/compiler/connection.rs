use super::{compile, BlockRequest, CompilerService, Endpoint, Reply};
use crate::{
    worker::{self, ChannelError, PoolHandle, ReceiverEndpoint, Sender, Transfer, WakePeer},
    ExecutionInterrupt, Memory, Result,
};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use relocade_x86::ExecutionTarget;

/// Lets the machine close and drain one CPU's connection to the compiler service.
pub(crate) struct Connection {
    requests: mpsc::UnboundedSender<Request>,
    finished: oneshot::Receiver<Result<()>>,
}

pub struct Request {
    pub target: ExecutionTarget,
    pub blocks: Vec<BlockRequest>,
}

#[derive(Transfer)]
pub(super) struct Start {
    memory: Memory,
    requests: mpsc::UnboundedReceiver<Request>,
    results: Sender<Reply>,
    peer: ReceiverEndpoint<WakePeer<ExecutionInterrupt>>,
    finished: oneshot::Sender<Result<()>>,
}

impl Connection {
    pub fn new(memory: Memory, compiler: &CompilerService) -> Result<(Self, Endpoint)> {
        let (requests, incoming) = mpsc::unbounded();
        let (results, replies) = worker::channel_endpoints()?;
        let (peer, wake) = worker::channel_endpoints()?;
        let (finished, completion) = oneshot::channel();
        compiler.submit(Start {
            memory,
            requests: incoming,
            results,
            peer: wake,
            finished,
        })?;
        Ok((
            Self {
                requests: requests.clone(),
                finished: completion,
            },
            Endpoint {
                requests,
                results: replies,
                peer,
            },
        ))
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.requests.close_channel();
        (&mut self.finished)
            .await
            .map_err(|_| ChannelError::Closed)?
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.requests.close_channel();
    }
}

impl Start {
    pub(super) async fn serve(
        mut self,
        compiler: &relocade_wasm_runtime::Compiler,
        jobs: &PoolHandle,
    ) {
        let mut peer = self.peer.bind();
        let result = async {
            let mut binding = None;
            while let Some(request) = self.requests.next().await {
                // Unused CPUs need no wake binding and cannot delay shutdown.
                if binding.is_none() {
                    binding = Some(peer.recv().await.ok_or(ChannelError::Closed)??.bind());
                }
                let result = compile(
                    compiler,
                    &self.memory,
                    request.target,
                    &request.blocks,
                    jobs,
                )
                .await;
                match self.results.send(Reply(result)) {
                    Ok(()) | Err(ChannelError::Closed) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        }
        .await;
        let _ = self.finished.send(result);
    }
}

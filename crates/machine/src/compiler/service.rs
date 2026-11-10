use super::{compile, BlockRequest, Endpoint, Reply};
use crate::{
    worker::{
        self, ChannelError, Receiver, ReceiverEndpoint, Sender, Transfer, WakeBinding, WakePeer,
        Worker,
    },
    ExecutionInterrupt, Memory, Result,
};
use futures_channel::mpsc;
use futures_util::{
    future::{select, Either},
    stream::{FuturesUnordered, Stream, StreamFuture},
    StreamExt,
};
use relocade_wasm_runtime::{Backend, WasmBackend};
use relocade_x86::ExecutionTarget;
use std::{
    pin::{pin, Pin},
    task::{Context, Poll},
};

/// The machine's single compiler worker and incoming request streams.
pub(crate) struct Service {
    inputs: Sender<Input>,
    worker: Worker<Result<()>>,
}

pub struct Request {
    pub target: ExecutionTarget,
    pub blocks: Vec<BlockRequest>,
}

#[derive(Transfer)]
struct Start {
    memory: Memory,
    inputs: ReceiverEndpoint<Input>,
}

#[derive(Transfer)]
struct Input {
    requests: mpsc::UnboundedReceiver<Request>,
    results: Sender<Reply>,
    peer: ReceiverEndpoint<WakePeer<ExecutionInterrupt>>,
}

struct Inbox {
    requests: mpsc::UnboundedReceiver<Request>,
    results: Sender<Reply>,
    peer: Receiver<WakePeer<ExecutionInterrupt>>,
    binding: Option<WakeBinding>,
}

impl Service {
    pub fn new(memory: Memory) -> Result<Self> {
        let compiler = Backend::new()?.compiler();
        let (inputs, incoming) = worker::channel_endpoints()?;
        let worker = Worker::spawn_with(
            Start {
                memory,
                inputs: incoming,
            },
            move |start, _stopped| serve(start, compiler),
        );
        Ok(Self { inputs, worker })
    }

    pub fn connect(&mut self) -> Result<Endpoint> {
        let (requests, incoming) = mpsc::unbounded();
        let (results, replies) = worker::channel_endpoints()?;
        let (peer, wake) = worker::channel_endpoints()?;
        self.inputs.send(Input {
            requests: incoming,
            results,
            peer: wake,
        })?;
        Ok(Endpoint {
            requests,
            results: replies,
            peer,
        })
    }

    pub async fn shutdown(self) -> Result<()> {
        let Self { inputs, worker } = self;
        drop(inputs);
        // Joining preserves startup so every posted inbox can be drained.
        worker.join().await?.transpose()?;
        Ok(())
    }
}

async fn serve(start: Start, compiler: relocade_wasm_runtime::Compiler) -> Result<()> {
    let mut inputs = start.inputs.bind();
    let mut pending = FuturesUnordered::<StreamFuture<Inbox>>::new();
    loop {
        let input = if pending.is_empty() {
            inputs.recv().await
        } else {
            match select(pin!(inputs.recv()), pending.next()).await {
                Either::Left((input, _)) => input,
                Either::Right((next, _)) => {
                    let (request, mut inbox) = next.expect("a pending request stream");
                    if let Some(request) = request {
                        inbox.reply(request, &start.memory, &compiler).await?;
                        pending.push(inbox.into_future());
                    }
                    continue;
                }
            }
        };
        let Some(input) = input else { break };
        let input = input?;
        pending.push(
            Inbox {
                requests: input.requests,
                results: input.results,
                peer: input.peer.bind(),
                binding: None,
            }
            .into_future(),
        );
    }

    // Closing a Rust receiver does not wake its pending poll. Reinsert each
    // stream so queued requests drain and idle streams finish without another send.
    let mut pending: FuturesUnordered<_> = pending
        .into_iter()
        .map(|next| {
            let mut inbox = next.into_inner().expect("a pending request stream");
            inbox.requests.close();
            inbox.into_future()
        })
        .collect();
    while let Some((request, mut inbox)) = pending.next().await {
        if let Some(request) = request {
            inbox.reply(request, &start.memory, &compiler).await?;
            pending.push(inbox.into_future());
        }
    }
    Ok(())
}

impl Inbox {
    async fn reply(
        &mut self,
        request: Request,
        memory: &Memory,
        compiler: &relocade_wasm_runtime::Compiler,
    ) -> Result<()> {
        // Unused CPUs need no wake binding and cannot delay shutdown.
        if self.binding.is_none() {
            let peer = self.peer.recv().await.ok_or(ChannelError::Closed)??;
            self.binding = Some(peer.bind());
        }
        let result = compile(compiler, memory, request.target, &request.blocks).await;
        match self.results.send(Reply(result)) {
            Ok(()) | Err(ChannelError::Closed) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl Stream for Inbox {
    type Item = Request;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Request>> {
        self.requests.poll_next_unpin(cx)
    }
}

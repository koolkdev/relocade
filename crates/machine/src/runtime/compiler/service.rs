use super::{receive_code, worker_error, BlockRequest, CompiledCode, CompilerWorker, Result};
use futures_channel::{mpsc, oneshot};
use futures_util::StreamExt;
use relocade_wasm_runtime::{
    worker::{self, Endpoint, Port},
    Module,
};
use relocade_x86::{BlockEntry, ExecutionTarget};
use std::future::poll_fn;

struct Request {
    route: usize,
    id: u64,
    blocks: Vec<BlockRequest>,
    reply: oneshot::Sender<Result<Vec<BlockEntry>>>,
}

/// Session-owned request coordination and module delivery. Create connections
/// before `run`, then drive it on a responsive host (the browser main thread).
/// The compiler worker belongs to this service, not to any execution owner.
pub struct CompilerService {
    worker: CompilerWorker,
    requests: mpsc::Sender<Request>,
    incoming: mpsc::Receiver<Request>,
    outputs: Vec<Port<(u64, Module)>>,
}

impl CompilerService {
    pub fn new(worker: CompilerWorker) -> Self {
        let (requests, incoming) = mpsc::channel(8);
        Self {
            worker,
            requests,
            incoming,
            outputs: Vec::new(),
        }
    }

    /// Capture the Rust connection in an execution closure and post its paired
    /// endpoint as context. Bind that pair on its final owner. Connections have
    /// independent lifetimes; dropping one does not close the others.
    pub fn connect(&mut self) -> Result<(CompilerConnection, Endpoint<(u64, Module)>)> {
        let (output, input) = worker::channel()?;
        let route = self.outputs.len();
        self.outputs.push(output.bind());
        Ok((
            CompilerConnection {
                requests: self.requests.clone(),
                route,
                target: self.worker.target,
            },
            input,
        ))
    }

    /// Drains accepted requests until all connections are dropped, then awaits
    /// compiler shutdown. Await this future through completion for cleanup.
    /// No CPU, hotness policy, code cache or automatic installation is retained.
    pub async fn run(self) -> Result<()> {
        let Self {
            mut worker,
            requests,
            mut incoming,
            outputs,
        } = self;
        drop(requests);
        while let Some(request) = incoming.next().await {
            if request.reply.is_canceled() {
                continue;
            }
            let compiled = worker.compile_blocks(request.blocks).await;
            if request.reply.is_canceled() {
                continue;
            }
            let result = compiled.and_then(|compiled| {
                outputs[request.route].send((request.id, compiled.module))?;
                Ok(compiled.entries)
            });
            // Readiness means the resource has already been posted to this CPU.
            // A disconnected route only fails its own request.
            let _ = request.reply.send(result);
        }
        worker.shutdown().await
    }
}

/// Sendable request connection, paired with the resource endpoint from `connect`.
pub struct CompilerConnection {
    requests: mpsc::Sender<Request>,
    route: usize,
    target: ExecutionTarget,
}

impl CompilerConnection {
    pub fn bind(self, endpoint: Endpoint<(u64, Module)>) -> CompilerClient {
        CompilerClient {
            connection: self,
            modules: endpoint.bind(),
            next_id: 0,
        }
    }
}

/// Local compilation client. It owns no compiler thread and is not a CPU field.
/// One exclusive waiter pairs Rust metadata with its posted module by request ID.
pub struct CompilerClient {
    connection: CompilerConnection,
    modules: Port<(u64, Module)>,
    next_id: u64,
}

impl CompilerClient {
    /// Waits for compilation and posting, using only Rust communication.
    /// Receiving the returned resource may require an event-loop turn. Use
    /// `Runner::wait_for_host(posted.receive())` alongside continuously busy work.
    /// Keep requested code and mappings stable until receipt, as required by
    /// [`CompilerWorker`]. Installation and later code validity remain the caller's
    /// responsibility; concurrent execution may change other guest data.
    pub async fn request(&mut self, blocks: Vec<BlockRequest>) -> Result<PostedCode<'_>> {
        poll_fn(|cx| self.connection.requests.poll_ready(cx))
            .await
            .map_err(worker_error)?;
        let id = self.next_id;
        self.next_id = id.checked_add(1).expect("compiler request ID exhausted");
        let (reply, result) = oneshot::channel();
        self.connection
            .requests
            .start_send(Request {
                route: self.connection.route,
                id,
                blocks,
                reply,
            })
            .map_err(worker_error)?;
        let entries = result.await.map_err(worker_error)??;
        Ok(PostedCode {
            modules: &mut self.modules,
            id,
            entries,
            target: self.connection.target,
        })
    }
}

/// Code is compiled and posted, but its module may still be awaiting delivery.
/// Dropping this value or its receive future abandons only this result.
pub struct PostedCode<'a> {
    modules: &'a mut Port<(u64, Module)>,
    id: u64,
    entries: Vec<BlockEntry>,
    target: ExecutionTarget,
}

impl PostedCode<'_> {
    pub async fn receive(self) -> Result<CompiledCode> {
        receive_code(self.modules, self.id, self.entries, self.target).await
    }
}

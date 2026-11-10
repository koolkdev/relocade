//! The runtime's first compilation publishes one connection and its shared memory.

use super::Runner;
use crate::{
    compiler::worker_error, worker, BlockRequest, CompiledCode, CompilerClient, CompilerConnection,
    CompilerWorker, Cpu, Error, Memory, Result,
};
use futures_util::lock::Mutex;
use relocade_x86::ExecutionTarget;
use std::cell::RefCell;

pub(super) struct CompilerInput {
    memory: Memory,
    target: ExecutionTarget,
    connection: CompilerConnection,
}

impl worker::WorkerResource for CompilerInput {
    type Posted = <(Memory, CompilerConnection) as worker::WorkerResource>::Posted;

    fn prepare(
        self,
    ) -> worker::ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send> {
        (self.memory, self.connection)
            .prepare()
            .map(move |(memory, connection)| Self {
                memory,
                connection,
                target: self.target,
            })
    }
}

impl CompilerInput {
    pub(super) async fn run(self) -> Result<()> {
        let mut worker = CompilerWorker::start(&self.memory, self.target)?;
        worker.connect(self.connection);
        worker.shutdown().await
    }
}

pub(super) struct Compilation {
    output: RefCell<Option<worker::Sender<CompilerInput>>>,
    source: RefCell<Option<(Memory, ExecutionTarget)>>,
    client: Mutex<Option<CompilerClient>>,
}

impl Compilation {
    pub(super) fn new(output: worker::Sender<CompilerInput>) -> Self {
        Self {
            output: RefCell::new(Some(output)),
            source: RefCell::new(None),
            client: Mutex::new(None),
        }
    }

    pub(super) fn register(&self, cpu: &Cpu) {
        *self.source.borrow_mut() = Some((cpu.memory(), cpu.cpu_state().execution_target()));
    }

    async fn compile(&self, runner: &Runner, blocks: Vec<BlockRequest>) -> Result<CompiledCode> {
        let mut client = self.client.lock().await;
        if client.is_none() {
            let (memory, target) = self.source.borrow().clone().ok_or_else(|| {
                worker_error("create the session CPU before requesting compilation")
            })?;
            let (local, connection) = runner.compiler_connection()?;
            self.output
                .borrow_mut()
                .as_mut()
                .expect("first compiler connection")
                .send(CompilerInput {
                    memory,
                    target,
                    connection,
                })
                .map_err(worker_error)?;
            self.output.borrow_mut().take();
            *client = Some(local);
        }
        client.as_mut().unwrap().compile(blocks).await
    }
}

impl Runner {
    /// Compiles explicit ranges from this session CPU's shared memory and target.
    /// Available inside `run_session` after `create_cpu`. The first request starts
    /// a persistent compiler; later requests reuse the connection. No companion
    /// task is needed. Code validity and installation remain caller-owned.
    pub async fn compile_blocks(&self, blocks: Vec<BlockRequest>) -> Result<CompiledCode> {
        let compilation = self
            .compilation
            .as_ref()
            .ok_or_else(|| Error::Worker("compilation requires a managed run_session".into()))?;
        compilation.compile(self, blocks).await
    }
}

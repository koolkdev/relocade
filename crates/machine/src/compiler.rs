//! A host-owned compiler thread with direct connections to execution workers.

mod client;
pub use client::{CompilerClient, CompilerConnection};

use crate::{Error, Memory, Result};
use futures_util::StreamExt;
use relocade_wasm_runtime::{
    worker::{ResourceError, SendTask, Thread, ThreadExt, WorkerResource},
    Backend, Module, WasmBackend, WasmCompiler,
};
use relocade_x86::{BlockEntry, BlockSnapshot, ExecutionTarget};

/// A bounded instruction range to read from the compiler's guest memory.
/// `address` is the guest physical address before A20 and physical mapping;
/// `start_eip` is the CS-relative entry exported by the generated block.
#[derive(Clone, Copy, Debug)]
pub struct BlockRequest {
    pub address: u32,
    pub start_eip: u32,
    pub byte_len: usize,
    pub instruction_limit: u32,
}

/// Prepared code and the instruction ranges used by its exported entries.
/// Each execution owner binds the module to its own CPU, memories and dispatch.
#[derive(WorkerResource)]
pub struct CompiledCode {
    #[worker(post)]
    pub module: Module,
    pub entries: Vec<BlockEntry>,
    pub execution_target: ExecutionTarget,
}

/// One compiler thread for shared guest memory and an execution target.
/// Attach independent connections with `connect`; each sends results directly
/// to its execution owner. Dropping a client closes only its connection.
///
/// Close the clients and await `shutdown` before releasing this owner. Posted
/// tasks start independently of polling shutdown; it only joins their cleanup.
/// Keep requested bytes and mappings stable until accepted work has finished.
pub struct CompilerWorker {
    thread: Thread,
    compiler: relocade_wasm_runtime::Compiler,
    memory: Memory,
    target: ExecutionTarget,
    tasks: Vec<SendTask<Result<()>>>,
}

impl CompilerWorker {
    pub fn start(memory: &Memory, target: ExecutionTarget) -> Result<Self> {
        Ok(Self {
            thread: Thread::new(),
            compiler: Backend::new()?.compiler(),
            memory: memory.clone(),
            target,
            tasks: Vec::new(),
        })
    }

    /// Attaches a request/response connection to this already-created worker.
    /// Memory and the result endpoint are posted once. Requests carry only ranges;
    /// success and failure share one ordered response stream with no readiness reply.
    pub fn connect(&mut self, connection: CompilerConnection) {
        let compiler = self.compiler.clone();
        let target = self.target;
        self.tasks.push(self.thread.run_resource(
            (self.memory.clone(), connection),
            move |(memory, mut connection)| async move {
                while let Some(blocks) = connection.incoming.next().await {
                    let result = compile(&compiler, &memory, target, &blocks).await;
                    match connection.results.send(result) {
                        Ok(()) | Err(ResourceError::Closed) => {}
                        Err(error) => return Err(worker_error(error)),
                    }
                }
                Ok(())
            },
        ));
    }

    /// Waits for every connection to drain accepted work and close. Clients must
    /// be dropped first, or be driven concurrently until they close. The thread
    /// stays alive through all task joins, including when one task fails.
    pub async fn shutdown(self) -> Result<()> {
        let mut result = Ok(());
        for task in self.tasks {
            let completed = task.await.map_err(worker_error).and_then(|result| result);
            if result.is_ok() {
                result = completed;
            }
        }
        result
    }
}

async fn compile(
    compiler: &relocade_wasm_runtime::Compiler,
    memory: &Memory,
    target: ExecutionTarget,
    requests: &[BlockRequest],
) -> Result<CompiledCode> {
    let captured: Vec<_> = requests
        .iter()
        .map(|block| memory.view().read_ram_rom(block.address, block.byte_len))
        .collect::<std::result::Result<_, _>>()?;
    let snapshots: Vec<_> = requests
        .iter()
        .zip(&captured)
        .map(|(block, bytes)| BlockSnapshot {
            start_eip: block.start_eip,
            bytes,
            instruction_limit: block.instruction_limit,
        })
        .collect();
    let blocks = relocade_x86::Compiler::new(target.profile)
        .with_cpu_model(target.cpu_model)
        .with_instruction_budgets(target.instruction_budgets)
        .compile_blocks(&snapshots)?;
    Ok(CompiledCode {
        module: compiler.compile(&blocks.bytes).await?,
        entries: blocks.entries,
        execution_target: blocks.execution_target,
    })
}

pub(crate) fn worker_error(error: impl std::fmt::Display) -> Error {
    Error::Worker(error.to_string())
}

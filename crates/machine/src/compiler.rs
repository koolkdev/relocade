//! Machine-owned compilation over shared guest memory.

mod client;
mod service;
pub use client::CompilerClient;
pub(crate) use client::Endpoint;
pub(crate) use service::Service;

use crate::{worker::Transfer, Cpu, DispatchEntry, Error, Memory, Result};
use relocade_wasm_runtime::{Module, WasmCompiler};
use relocade_x86::{BlockEntry, BlockSnapshot, ExecutionTarget};

/// Creates the application's shared compilation pool using half the available
/// logical parallelism, rounded down, with at least one worker.
/// Drive the pool alongside machine lifecycles, including their shutdowns.
/// For an explicit limit, use [`crate::worker::Pool::new`] with supplied threads.
pub fn compilation_pool() -> Result<(crate::worker::Pool, crate::worker::PoolHandle)> {
    let parallelism = crate::worker::available_parallelism().map_or(1, usize::from);
    let threads = (0..(parallelism / 2).max(1))
        .map(|_| crate::worker::Thread::new())
        .collect();
    Ok(crate::worker::Pool::new(threads)?)
}

/// An instruction range at the dispatch linear address `CS.base + start_eip`.
/// On the supported Real16 target this is also the physical address before A20.
/// Keep bytes and mappings stable until its result arrives or the machine shuts down.
#[derive(Clone, Copy, Debug)]
pub struct BlockRequest {
    pub address: u32,
    pub start_eip: u32,
    pub byte_len: usize,
    pub instruction_limit: u32,
}

/// Compiled entries awaiting explicit installation on a compatible, stopped CPU.
/// Clones share the engine module. Each installation creates CPU-local bindings.
#[derive(Clone)]
pub struct CompiledCode {
    module: Module,
    description: Description,
}

#[derive(Clone)]
pub struct Description {
    addresses: Vec<u32>,
    blocks: Vec<BlockEntry>,
    target: ExecutionTarget,
}

impl CompiledCode {
    pub fn blocks(&self) -> &[BlockEntry] {
        &self.description.blocks
    }

    /// Publishes these entries through the CPU's existing dispatch and hook path.
    /// The caller maintains code/mapping validity for the installed code's lifetime.
    /// Pending or parked host calls must finish before installation.
    pub async fn install(&self, cpu: &mut Cpu) -> Result<()> {
        if cpu.cpu_state().execution_target() != self.description.target {
            return Err(Error::CompilationTarget);
        }
        let entries: Vec<_> = self
            .description
            .addresses
            .iter()
            .zip(&self.description.blocks)
            .map(|(&address, block)| DispatchEntry {
                address,
                export: &block.export,
            })
            .collect();
        cpu.install_compiled_module(&self.module, &entries).await
    }
}

/// Successes and errors occupy the same ordered channel message.
pub struct Reply(Result<CompiledCode>);

impl Transfer for Reply {
    type State = Result<Description>;
    type Payload = std::result::Result<Module, ()>;

    fn encode(self) -> (Self::State, Self::Payload) {
        match self.0 {
            Ok(code) => (Ok(code.description), Ok(code.module)),
            Err(error) => (Err(error), Err(())),
        }
    }

    fn decode(state: Self::State, payload: Self::Payload) -> Self {
        Self(match (state, payload) {
            (Ok(description), Ok(module)) => Ok(CompiledCode {
                module,
                description,
            }),
            (Err(error), Err(())) => Err(error),
            _ => unreachable!("compilation state and payload must travel together"),
        })
    }
}

async fn compile(
    compiler: &relocade_wasm_runtime::Compiler,
    memory: &Memory,
    target: ExecutionTarget,
    requests: &[BlockRequest],
    jobs: &crate::worker::PoolHandle,
) -> Result<CompiledCode> {
    let captured: Vec<_> = requests
        .iter()
        .map(|block| memory.read_code(block.address, block.byte_len))
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
        .compile_blocks_with(&snapshots, jobs)
        .await?;
    Ok(CompiledCode {
        module: compiler.compile(&blocks.bytes).await?,
        description: Description {
            addresses: requests.iter().map(|block| block.address).collect(),
            blocks: blocks.entries,
            target: blocks.execution_target,
        },
    })
}

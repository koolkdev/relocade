//! Shared compilation service over each machine's guest memory.

mod client;
mod code;
pub use code::{CompiledCode, Description};
mod connection;
mod service;
pub use client::CompilerClient;
pub(crate) use client::Endpoint;
pub(crate) use connection::Connection;
pub use service::CompilerService;

use crate::{worker::Transfer, FarPointer, Memory, Result};
use relocade_wasm_runtime::{Module, WasmCompiler};
use relocade_x86::{BlockSnapshot, ExecutionProfile, ExecutionTarget, FunctionLimits};

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

/// One clean real-mode function candidate. Its blocks are discovered by x86 on
/// the compiler service, without reading code on the submitting CPU.
/// Interior host hooks and mutable code require separate admission/invalidation.
#[derive(Clone, Copy, Debug)]
pub struct FunctionRequest {
    pub entry: FarPointer,
    pub limits: FunctionLimits,
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
    request: connection::Compilation,
    jobs: &crate::worker::PoolHandle,
) -> Result<CompiledCode> {
    let x86 = relocade_x86::Compiler::new(target.profile)
        .with_cpu_model(target.cpu_model)
        .with_instruction_budgets(target.instruction_budgets);
    let (blocks, addresses) = match request {
        connection::Compilation::Blocks(requests) => {
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
            let blocks = x86.compile_blocks_with(&snapshots, jobs).await?;
            let addresses = requests.iter().map(|block| block.address).collect();
            (blocks, addresses)
        }
        connection::Compilation::Functions(requests) => {
            if target.profile != ExecutionProfile::Real16 {
                return Err(crate::ConfigurationError::UnsupportedMode.into());
            }
            let mut module = x86.function_module();
            let mut addresses = Vec::new();
            let mut first_error = None;
            for request in requests {
                let base = request.entry.segment_base();
                match module.add_function(
                    u32::from(request.entry.offset),
                    request.limits,
                    |eip, buffer| {
                        let len = buffer.len().min(0x10000_u32.saturating_sub(eip) as usize);
                        if len == 0 {
                            return 0;
                        }
                        memory.read_code_prefix(base + eip, &mut buffer[..len])
                    },
                ) {
                    Ok(entries) => {
                        addresses.extend(entries.iter().map(|block| base + block.start_eip));
                    }
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            if addresses.is_empty() {
                if let Some(error) = first_error {
                    return Err(error.into());
                }
            }
            (module.compile_with(jobs).await?, addresses)
        }
    };
    Ok(CompiledCode {
        module: compiler.compile(&blocks.bytes).await?,
        description: Description {
            ranges: code::ranges(&addresses, &blocks.entries),
            addresses,
            blocks: blocks.entries,
            target: blocks.execution_target,
        },
    })
}

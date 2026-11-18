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
use relocade_x86::{
    BlockSnapshot, CodeAddress, ExecutionProfile, ExecutionTarget, FunctionLimits,
    SegmentDefaultSize,
};

/// One real-mode CS:IP and the bounds of the bytes to capture at that address.
/// Keep bytes and mappings stable until its result arrives or the machine shuts down.
#[derive(Clone, Copy, Debug)]
pub struct BlockRequest {
    pub entry: FarPointer,
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
    jobs: &crate::worker::JobExecutor,
) -> Result<CompiledCode> {
    if target.profile != ExecutionProfile::Real16 {
        return Err(crate::ConfigurationError::UnsupportedMode.into());
    }
    let mut reader = memory.code_reader();
    let x86 = relocade_x86::Compiler::new(target.profile)
        .with_cpu_model(target.cpu_model)
        .with_instruction_budgets(target.instruction_budgets);
    let blocks = match request {
        connection::Compilation::Blocks(requests) => {
            let captured: Vec<_> = requests
                .iter()
                .map(|block| reader.read(block.entry.address(), block.byte_len))
                .collect::<std::result::Result<_, _>>()?;
            let snapshots: Vec<_> = requests
                .iter()
                .zip(&captured)
                .map(|(block, bytes)| BlockSnapshot {
                    address: block.entry.into(),
                    code_default_size: SegmentDefaultSize::Bits16,
                    bytes,
                    instruction_limit: block.instruction_limit,
                })
                .collect();
            x86.compile_blocks_with(&snapshots, jobs).await?
        }
        connection::Compilation::Functions(requests) => {
            let mut module = x86.function_module();
            let mut has_blocks = false;
            let mut first_error = None;
            for request in requests {
                let base = request.entry.segment_base();
                match module.add_function(
                    request.entry.into(),
                    SegmentDefaultSize::Bits16,
                    request.limits,
                    |eip, buffer| {
                        let len = buffer.len().min(0x10000_u32.saturating_sub(eip) as usize);
                        if len == 0 {
                            return 0;
                        }
                        reader.read_prefix(base + eip, &mut buffer[..len])
                    },
                ) {
                    Ok(entries) => has_blocks |= !entries.is_empty(),
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            if !has_blocks {
                if let Some(error) = first_error {
                    return Err(error.into());
                }
            }
            module.compile_with(jobs).await?
        }
    };
    let source = reader.finish(blocks.entries.iter().map(|block| {
        let CodeAddress::RealMode { segment, eip } = block.address else {
            unreachable!("the machine supplies a real-mode CS for every entry")
        };
        ((u32::from(segment) << 4).wrapping_add(eip), block.byte_len)
    }));
    Ok(CompiledCode::new(
        compiler.compile(&blocks.bytes).await?,
        blocks,
        source,
    ))
}

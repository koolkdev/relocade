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
    BlockSnapshot, CodeAddress, DiscoveryLimits, ExecutionProfile, ExecutionTarget, Segment,
    SegmentDefaultSize, SegmentRegister,
};

/// One real-mode CS:IP and the bounds of the bytes to capture at that address.
/// Keep bytes and mappings stable until its result arrives or the machine shuts down.
#[derive(Clone, Copy, Debug)]
pub struct BlockRequest {
    pub entry: FarPointer,
    pub byte_len: usize,
    pub instruction_limit: u32,
}

/// A real-mode guest function entry to compile.
/// The limits bound the new instructions and bytes discovered from this entry.
#[derive(Clone, Copy, Debug)]
pub struct FunctionRequest {
    pub entry: FarPointer,
    pub limits: DiscoveryLimits,
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
            let mut discovery = x86.discover_blocks(|address, bytes| {
                reader.read_segment_prefix(&code_segment(address), address.eip(), bytes)
            });
            let mut first_error = None;
            for request in requests {
                if let Err(error) = discovery.add_root(
                    request.entry.into(),
                    SegmentDefaultSize::Bits16,
                    request.limits,
                ) {
                    first_error.get_or_insert(error);
                }
            }
            if discovery.is_empty() {
                if let Some(error) = first_error {
                    return Err(error.into());
                }
            }
            discovery.compile_with(jobs).await?
        }
    };
    let source = reader.finish(
        blocks
            .spans
            .iter()
            .map(|&(address, len)| (code_segment(address).base + address.eip(), len)),
    );
    Ok(CompiledCode::new(
        compiler.compile(&blocks.bytes).await?,
        blocks,
        source,
    ))
}

fn code_segment(address: CodeAddress) -> SegmentRegister {
    let CodeAddress::RealMode { segment, .. } = address else {
        unreachable!("the machine supplies a real-mode CS for every entry")
    };
    SegmentRegister::real_mode(Segment::Cs, segment)
}

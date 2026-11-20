//! Shared compilation service over each machine's guest memory.

mod client;
mod code;
pub use code::{CompilationBinding, CompiledCode, Description};
mod connection;
mod discovery;
mod service;
pub use client::CompilerClient;
pub(crate) use client::Endpoint;
pub(crate) use connection::Connection;
pub use discovery::{DiscoveryError, DiscoveryLimits};
pub use service::CompilerService;

use crate::{memory::code::CodeReader, worker::Transfer, FarPointer, Memory, Result};
use relocade_wasm_runtime::{Module, WasmCompiler};
use relocade_x86::{
    BlockSnapshot, CodeAddress, CompilerConfig, DecodedBlock, Segment, SegmentDefaultSize,
    SegmentRegister,
};

/// One real-mode CS:IP and the bounds of the bytes to capture at that address.
/// Installation checks the bytes used by the emitted instructions.
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
    config: CompilerConfig,
    request: connection::Compilation,
    jobs: &crate::worker::JobExecutor,
) -> Result<CompiledCode> {
    memory.configuration().validate_compiler_config(config)?;
    let mut reader = memory.code_reader();
    let x86 = relocade_x86::Compiler::from_config(config);
    let decoded = match request {
        connection::Compilation::Blocks(requests) => capture_blocks(&x86, &requests, &mut reader)?,
        connection::Compilation::Functions(requests) => {
            discover_functions(config, requests, &mut reader)?
        }
    };

    let mut compiled = Vec::with_capacity(decoded.len());
    let mut interpreted = Vec::new();
    for block in decoded {
        let address = block.address();
        let cs = code_segment(address);
        if !reader.is_excluded(cs.base + address.eip(), block.byte_len()) {
            compiled.push(block);
        } else if block.is_exported() {
            if let Ok(offset) = u16::try_from(address.eip()) {
                interpreted.push(FarPointer {
                    segment: cs.selector,
                    offset,
                });
            }
        }
    }
    let blocks = x86.compile_decoded_blocks_with(compiled, jobs).await?;
    let source = reader.finish(
        blocks
            .spans
            .iter()
            .map(|&(address, len)| (code_segment(address).base + address.eip(), len)),
    );
    CompiledCode::new(
        compiler.compile(&blocks.bytes).await?,
        blocks,
        source,
        interpreted,
    )
}

fn capture_blocks(
    x86: &relocade_x86::Compiler,
    requests: &[BlockRequest],
    reader: &mut CodeReader<'_>,
) -> Result<Vec<DecodedBlock>> {
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
    Ok(x86.decode_blocks(&snapshots)?)
}

/// Plan a batch over the worker's retained snapshots. A rejected candidate
/// leaves other roots eligible, including their shared continuations.
pub(crate) fn discover_functions(
    config: CompilerConfig,
    requests: impl IntoIterator<Item = FunctionRequest>,
    reader: &mut CodeReader<'_>,
) -> Result<Vec<DecodedBlock>> {
    let mut discovery = discovery::BlockDiscovery::new(config, |address, bytes| {
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
    let blocks = discovery.finish()?;
    if blocks.is_empty() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(blocks)
}

fn code_segment(address: CodeAddress) -> SegmentRegister {
    let CodeAddress::RealMode { segment, .. } = address else {
        unreachable!("the machine supplies a real-mode CS for every entry")
    };
    SegmentRegister::real_mode(Segment::Cs, segment)
}

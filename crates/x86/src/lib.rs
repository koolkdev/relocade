//! Compiles bounded x86 instruction snapshots to WebAssembly.
//!
//! Currently supported instructions are 32-bit immediate-to-register MOVs
//! (`B8` through `BF`, followed by four little-endian immediate bytes).
//!
//! ```
//! use wasm86_x86::compile_block_from_bytes;
//!
//! // MOV EAX, 42; MOV ECX, 7.
//! let block = compile_block_from_bytes(
//!     0x1000,
//!     &[0xb8, 42, 0, 0, 0, 0xb9, 7, 0, 0, 0],
//!     2,
//! )?;
//! assert_eq!(block.entry, "block_1000");
//! # Ok::<(), wasm86_x86::BlockError>(())
//! ```
#![forbid(unsafe_code)]

mod decode;
mod semantics;
mod state;

use std::fmt;

use wasm86_compiler::{FunctionImport, Program, Signature, Type, I32};

/// A WebAssembly module and the exported function that enters its block.
pub struct CompiledBlock {
    pub bytes: Vec<u8>,
    pub entry: String,
}

/// A failure to construct a block, not an exception raised by guest execution.
#[derive(Debug, Eq, PartialEq)]
pub enum BlockError {
    ZeroInstructionLimit,
    /// A selected instruction is incomplete. `available` counts the snapshot
    /// bytes remaining from that instruction's start, including its opcode.
    TruncatedInstruction {
        address: u32,
        available: usize,
    },
    /// The selected opcode is outside the supported instruction subset.
    UnsupportedOpcode {
        address: u32,
        opcode: u8,
    },
    Compiler(wasm86_compiler::BuildError),
}

impl fmt::Display for BlockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroInstructionLimit => {
                formatter.write_str("a block must contain at least one instruction")
            }
            Self::TruncatedInstruction { address, available } => write!(
                formatter,
                "incomplete instruction at {address:#x}: {available} snapshot bytes remain"
            ),
            Self::UnsupportedOpcode { address, opcode } => {
                write!(
                    formatter,
                    "unsupported opcode {opcode:#04x} at {address:#x}"
                )
            }
            Self::Compiler(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BlockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compiler(error) => Some(error),
            _ => None,
        }
    }
}

impl From<wasm86_compiler::BuildError> for BlockError {
    fn from(error: wasm86_compiler::BuildError) -> Self {
        Self::Compiler(error)
    }
}

/// Compiles exactly `instruction_limit` instructions starting at `start_eip`.
/// Bytes after that selection are ignored. Missing or unsupported selected bytes
/// are construction errors. This byte-only input carries no guest-fault information.
/// EIP and the completed-instruction count advance with 32-bit wrapping arithmetic.
///
/// The exported `block_<hex start_eip>` function has signature `() -> i64` and
/// imports `wasm86.cpuState`, a memory of at least one 64-KiB page. Its little-endian
/// 32-bit fields are EAX, ECX, EDX, EBX, ESP, EBP, ESI and EDI at offsets 24 through
/// 52 in steps of four, EIP at 56 and the completed-instruction count at 144.
/// Other bytes are preserved. Final register values are written in first-write
/// order, followed by EIP and count. The block then tail-calls the imported
/// `wasm86.dispatch(i32) -> i64` with the next EIP and returns its result.
pub fn compile_block_from_bytes(
    start_eip: u32,
    bytes: &[u8],
    instruction_limit: u32,
) -> Result<CompiledBlock, BlockError> {
    if instruction_limit == 0 {
        return Err(BlockError::ZeroInstructionLimit);
    }

    let mut program = Program::new();
    let mut state = state::State::new(&mut program);
    let dispatch = program.import_function(FunctionImport {
        module: "wasm86".into(),
        name: "dispatch".into(),
        signature: Signature {
            parameters: vec![Type::I32],
            result: Type::I64,
        },
    });
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I64,
    });
    let mut body = program.define(function)?;
    let mut remaining = bytes;
    let mut next_eip = start_eip;

    for _ in 0..instruction_limit {
        let (destination, immediate) = decode::mov32(remaining, next_eip)?;
        let source = body.constant::<I32>(immediate);
        semantics::mov32(&mut state, destination, &source);
        remaining = &remaining[5..];
        next_eip = next_eip.wrapping_add(5);
    }

    let next = body.constant::<I32>(next_eip);
    state.publish(&mut body, &next, instruction_limit)?;
    body.tail_call(dispatch, &[next.argument()])?;
    let entry = format!("block_{start_eip:x}");
    program.export(&entry, function)?;
    Ok(CompiledBlock {
        bytes: program.compile()?,
        entry,
    })
}

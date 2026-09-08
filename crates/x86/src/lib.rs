//! Builds WebAssembly execution entries for a small x86 instruction subset.
//!
//! Supports byte MOV (`B0`–`B7`, `88`, `8A`) and dword MOV (`B8`–`BF`, `89`, `8B`)
//! with immediate, register and memory operands. ModRM/SIB addresses are 32-bit.
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

mod address;
mod block;
mod decode;
mod execution;
mod fetch;
mod instruction;
mod interpreter;
mod memory;
mod register;
mod semantics;
mod ssa;
mod state;

#[cfg(test)]
#[path = "../tests/support/step.rs"]
mod test_step;

use std::fmt;

use wasm86_compiler::{Func, FunctionImport, Program, Signature, Type};

pub use block::compile_block_from_bytes;
pub use interpreter::compile_interpreter_step;

/// A WebAssembly module and the exported function that enters it.
pub struct CompiledModule {
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

fn declare_dispatch(program: &mut Program) -> Func {
    program.import_function(FunctionImport {
        module: "wasm86".into(),
        name: "dispatch".into(),
        signature: Signature {
            parameters: vec![Type::I32],
            result: Type::I64,
        },
    })
}

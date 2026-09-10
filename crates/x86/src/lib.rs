//! Builds WebAssembly execution entries for a small x86 instruction subset.
//!
//! Supports byte, word and dword MOV, ADD, ADC, SUB, SBB, CMP, AND, OR, XOR, TEST,
//! INC, DEC, NEG and NOT, plus byte SETcc (`0F 90`–`0F 9F`). Binary families include register,
//! register/memory and immediate forms. TEST supports `84`/`85`, `A8`/`A9` and
//! `F6`/`F7` /0. Group `83` sign-extends its byte immediate to the operand width.
//! INC/DEC use `FE`/`FF` /0 and /1, or opcode-selected word/dword registers `40`–`4F`.
//! NOT and NEG use `F6`/`F7` /2 and /3. These unary forms have no immediate.
//! Word/dword PUSH and POP use `50`–`5F`, `FF` /6 and `8F` /0. PUSH also accepts
//! an operand-sized immediate (`68`) or a sign-extended byte (`6A`). The stack
//! pointer is always 32-bit: PUSH reads its source before decrementing ESP;
//! POP uses the incremented ESP to address a memory destination. POP ESP replaces
//! the pointer with the popped dword; POP SP preserves the incremented high word.
//! MOVZX (`0F B6`/`0F B7`) and MOVSX (`0F BE`/`0F BF`) read a byte/word
//! register or memory source into a dword destination, or a word with `66`.
//! They zero-extend or sign-extend from the opcode's fixed source width. The
//! word-to-word `66 0F B7`/`66 0F BF` forms copy the source unchanged.
//! CMOVcc (`0F 40`–`0F 4F`) conditionally copies a word/dword register or memory
//! source into a register. Its source is read even when the condition is false.
//! A false condition preserves the destination; a taken word move preserves its
//! upper half. Both outcomes retire once and preserve flags.
//! Relative JMP uses `EB`/`E9`; Jcc uses `70`–`7F`/`0F 80`–`0F 8F`.
//! Short displacements are signed bytes; near displacements are word/dword-sized.
//! Targets are relative to the end of the instruction. With `66`, taken targets
//! are truncated to sixteen bits even for short branches; an untaken Jcc retains
//! the full 32-bit fallthrough EIP. Branches retire once and dispatch without
//! fetching the destination instruction. Snapshot blocks end at the first branch
//! or the requested instruction limit, whichever comes first.
//! Each full memory access is checked before effects, source first. A fault
//! preserves the current instruction's entry state and publishes earlier progress.
//! ModRM/SIB effective addresses and absolute offsets are 32-bit, independent
//! of the data width.
//! In this default-32 mode, `66` selects word operands; repetition has the same
//! effect and byte forms remain byte-sized. Other prefixes, including address-size
//! `67`, are outside the subset. Instructions contain at most fifteen bytes,
//! including prefixes and all required operand fields.
//!
//! Binary arithmetic, logic and NEG replace all six status flags. INC/DEC preserve
//! CF and update the other five; MOV, MOVZX, MOVSX, CMOVcc, NOT, PUSH, POP, SETcc and
//! branches preserve them all.
//! CMP and TEST only change flags. The CPU
//! stores flags lazily: byte 0 selects the record kind, and little-endian dwords
//! at 4 and 8 hold the original, zero-extended operands. SUB kinds are 1, 5 and 9;
//! ADD kinds are 2, 6 and 10, for byte, word and dword operations respectively.
//! Logic records use kinds 3, 7 and 11 with the result at offset 4; offset 8 is unused.
//! They clear CF/OF. For architecturally undefined AF, wasm86 chooses zero to avoid
//! retaining the old flag source; this is an implementation policy, not an x86
//! guarantee. Undefined flags remain ordinary readable bits. A nonzero kind owns all six
//! status flags, so their concrete bytes may be stale. Kind 0 instead reads the
//! concrete CF/PF/AF/ZF/SF/OF bytes at offsets 12 through 17, each containing 0 or 1.
//! ADC adds the incoming CF; SBB subtracts it as a borrow. Their local sources
//! retain the result and six explicit symbolic flag values. At publication,
//! they write all six concrete flags before kind 0, leaving unused payloads intact.
//! INC/DEC publish through the same concrete format; NEG uses SUB with a zero left operand.
//! Valid record kinds are an internal invariant. Flag reads preserve the
//! record, and these instructions leave non-status flag bytes untouched.
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
mod flags;
mod instruction;
mod interpreter;
mod memory;
mod register;
mod ssa;
mod state;

#[cfg(test)]
#[path = "../tests/support/step.rs"]
mod test_step;

use std::fmt;

use wasm86_compiler::{Func, FunctionImport, Program, Signature, Type};

pub use block::compile_block_from_bytes;
pub use interpreter::compile_interpreter_step;
pub use register::Gpr32;
pub use state::{CpuState, Registers, StatusFlags, StoredFlags};

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
    /// bytes remaining from that instruction's start, including any prefixes.
    TruncatedInstruction {
        address: u32,
        available: usize,
    },
    /// Decoding requires a byte beyond the fifteen-byte instruction limit.
    /// No byte after that limit is consumed.
    InstructionTooLong {
        address: u32,
    },
    /// The selected encoding is outside the supported instruction subset.
    /// `opcode` is the first byte after any `66` prefixes; other fields may
    /// select an unsupported form. Extended opcodes report `0F` here.
    UnsupportedInstruction {
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
            Self::InstructionTooLong { address } => {
                write!(
                    formatter,
                    "instruction at {address:#x} exceeds fifteen bytes"
                )
            }
            Self::UnsupportedInstruction { address, opcode } => {
                write!(
                    formatter,
                    "unsupported instruction at {address:#x} (opcode {opcode:#04x})"
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
            result: Some(Type::I64),
        },
    })
}

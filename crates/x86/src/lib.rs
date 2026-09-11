//! Builds WebAssembly execution entries for a small x86 instruction subset.
//!
//! Supports byte, word and dword MOV, ADD, ADC, SUB, SBB, CMP, AND, OR, XOR, TEST,
//! INC, DEC, NEG and NOT, plus byte SETcc (`0F 90`–`0F 9F`). Binary families include register,
//! register/memory and immediate forms. TEST supports `84`/`85`, `A8`/`A9` and
//! `F6`/`F7` /0. Group `83` sign-extends its byte immediate to the operand width.
//! INC/DEC use `FE`/`FF` /0 and /1, or opcode-selected word/dword registers `40`–`4F`.
//! NOT and NEG use `F6`/`F7` /2 and /3. These unary forms have no immediate.
//! MUL and IMUL use `F6`/`F7` /4 and /5 to multiply AL, AX or EAX by a same-width
//! source into AX, DX:AX or EDX:EAX. IMUL also supports word/dword two-operand
//! `0F AF` and three-operand `69`/`6B`; `6B` sign-extends its immediate byte.
//! CF and OF indicate that the unsigned or signed product does not fit the input
//! width; PF/AF/ZF/SF are undefined and use the policy 1/0/0/0. All source reads
//! precede result writes, and memory sources require only read permission.
//! DIV and IDIV use `F6`/`F7` /6 and /7 to divide AX, DX:AX or EDX:EAX by a byte,
//! word or dword register/memory divisor, respectively. Quotient and remainder
//! replace AL/AH, AX/DX or EAX/EDX. IDIV truncates toward zero; a nonzero remainder has the
//! dividend's sign. Zero divisors and out-of-range quotients raise divide error
//! before either result changes. All six status flags are undefined on success;
//! wasm86 preserves their incoming record. Memory sources are read before any
//! arithmetic fault check and need only read permission.
//! SHL/SAL, SHR and SAR use group extensions /4, /5 and /7. Byte forms use D0/D2/C0
//! for counts one/CL/imm8; word/dword forms use D1/D3/C1. Counts are masked with 31.
//! Zero preserves value and flags, but memory still requires full write permission.
//! SAR repeats the logical sign bit. CL is read before writing an overlapping destination.
//! SHLD (`0F A4`/`0F A5`) and SHRD (`0F AC`/`0F AD`) shift a word/dword
//! register/memory destination while filling from a same-width register. Their
//! third operand is imm8 or CL, masked with 31. Both sources use entry register
//! values. Zero preserves the destination and flags with the same full write check.
//! BT/BTS/BTR/BTC use `0F A3`/`AB`/`B3`/`BB` with a register bit offset, or
//! `0F BA` /4–/7 with imm8, for word/dword register or memory operands. Register
//! destinations and immediate offsets take the index modulo 16 or 32. A memory
//! register offset is signed at that width and selects a unit of the bit string;
//! its byte displacement wraps at 32 bits before the usual full-unit checks.
//! BT only reads; BTS/BTR/BTC require full write access even for an unchanged bit.
//! Both the offset and address use register values from before the instruction.
//! Word/dword PUSH and POP use `50`–`5F`, `FF` /6 and `8F` /0. PUSH also accepts
//! an operand-sized immediate (`68`) or a sign-extended byte (`6A`). The stack
//! pointer is always 32-bit: PUSH reads its source before decrementing ESP;
//! POP uses the incremented ESP to address a memory destination. POP ESP replaces
//! the pointer with the popped dword; POP SP preserves the incremented high word.
//! MOVZX (`0F B6`/`0F B7`) and MOVSX (`0F BE`/`0F BF`) read a byte/word
//! register or memory source into a dword destination, or a word with `66`.
//! They zero-extend or sign-extend from the opcode's fixed source width. The
//! word-to-word `66 0F B7`/`66 0F BF` forms copy the source unchanged.
//! CBW (`66 98`) sign-extends AL into AX; CWDE (`98`) sign-extends AX into EAX.
//! CWD (`66 99`) fills DX with the sign of AX; CDQ (`99`) fills EDX with the sign
//! of EAX. CWD/CDQ preserve the input accumulator, and word destinations preserve
//! their parent's upper half. These opcode-only forms preserve every flag.
//! LEA (`8D`) writes a ModRM/SIB effective address to a dword register, or its
//! low word with `66`. It reads full 32-bit address registers, preserves flags,
//! and performs no data-memory access. Register-mode ModRM is unsupported.
//! XCHG (`86`/`87`) exchanges a byte/word/dword register with a register or memory
//! operand. `90`–`97` exchange AX/EAX with an opcode-selected register; `90` and
//! `66 90` are NOP aliases. Both old values and any address use the entry register
//! state. Memory requires full write permission before either operand changes.
//! Memories are unshared; concurrent shared-memory synchronization is outside this ABI.
//! XADD (`0F C0`/`0F C1`) writes the sum to its register/memory destination and
//! the old destination to its source register, with ADD flags. CMPXCHG (`0F B0`/`0F B1`)
//! compares AL/AX/EAX with its destination, with subtraction flags. Equality writes
//! the source register to the destination; mismatch copies the old destination to
//! the accumulator. Both support byte/word/dword operands and check full memory
//! write permission before any effects, including a mismatching CMPXCHG.
//! CMOVcc (`0F 40`–`0F 4F`) conditionally copies a word/dword register or memory
//! source into a register. Its source is read even when the condition is false.
//! A false condition preserves the destination; a taken word move preserves its
//! upper half. Both outcomes retire once and preserve flags.
//! Relative JMP uses `EB`/`E9`; Jcc uses `70`–`7F`/`0F 80`–`0F 8F`.
//! Short displacements are signed bytes; near displacements are word/dword-sized.
//! Targets are relative to the end of the instruction. With `66`, taken targets
//! are truncated to sixteen bits even for short branches; an untaken Jcc retains
//! the full 32-bit fallthrough EIP. Near CALL (`E8` relative, `FF` /2 indirect)
//! reads its target before pushing the fallthrough pointer at operand width.
//! Indirect JMP (`FF` /4) reads an absolute target without changing ESP.
//! Near RET (`C3`) pops its target; `C2` then adds an unsigned imm16 cleanup
//! byte count to full ESP. Only the return-pointer cell is accessed. Word targets
//! are zero-extended, and word CALL saves the low fallthrough pointer.
//! CALL and RET preserve all registers except ESP and preserve every flag.
//! Far transfers remain outside the subset. Transfers retire once and dispatch without
//! fetching the destination instruction. Snapshot blocks end at the first control transfer
//! or the requested instruction limit, whichever comes first.
//! Each full memory access is checked before instruction effects. A fault
//! preserves the current instruction's entry state and publishes earlier progress.
//! ModRM/SIB effective addresses and absolute offsets are 32-bit, independent
//! of the data width.
//! In this default-32 mode, `66` selects word operands; repetition has the same
//! effect and byte forms remain byte-sized. Other prefixes, including address-size
//! `67`, are outside the subset. Instructions contain at most fifteen bytes,
//! including prefixes and all required operand fields.
//!
//! Binary arithmetic, logic, NEG, XADD and CMPXCHG replace all six status flags. INC/DEC preserve
//! CF and update the other five; MOV, MOVZX, MOVSX, CBW, CWDE, CWD, CDQ,
//! LEA, XCHG, CMOVcc, NOT, PUSH,
//! POP, CALL, RET, SETcc and jumps preserve them all.
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
//! ADC adds the incoming CF; SBB subtracts it as a borrow. Their ALU outcomes
//! contain the result and six explicit symbolic flag values. At publication,
//! they write all six concrete flags before kind 0, leaving unused payloads intact.
//! INC/DEC publish through the same concrete format; NEG uses SUB with a zero left operand.
//! Nonzero shifts publish concrete flags through that format too. PF/ZF/SF describe
//! the result and CF the last shifted-out bit, except SHL/SHR CF is undefined at
//! counts at or above the operand width. OF is defined only at count one: result
//! sign XOR CF for SHL, original sign for SHR, zero for SAR. AF is undefined for
//! nonzero counts. wasm86 chooses zero for undefined CF/OF/AF. Zero-count shifts
//! preserve the previous source, including an earlier instruction's pending flags.
//! SHLD/SHRD keep CF defined through the operand width; OF at one compares the
//! old and new sign. Word counts 17–31 leave result and all six flags undefined:
//! wasm86 chooses a zero result, zero CF/AF/OF and PF/ZF/SF from that result.
//! BT/BTS/BTR/BTC put the old selected bit into CF and leave ZF unchanged.
//! OF/SF/AF/PF are undefined; wasm86 preserves their prior logical values.
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
mod alu;
mod block;
mod decode;
mod execution;
mod instruction;
mod interpreter;
mod memory;
mod register;
mod ssa;
mod state;

#[cfg(test)]
extern crate self as wasm86_x86;

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod support;

#[cfg(test)]
use support::step as test_step;

#[cfg(test)]
#[path = "../tests/instructions.rs"]
mod instruction_tests;

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
            results: vec![Type::I64],
        },
    })
}

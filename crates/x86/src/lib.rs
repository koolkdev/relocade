//! Builds WebAssembly execution entries for a small x86 instruction subset.
//!
//! Encoding examples assume 32-bit code defaults unless stated otherwise.
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
//! its byte displacement wraps at address size before the usual full-unit checks.
//! BT only reads; BTS/BTR/BTC require full write access even for an unchanged bit.
//! Both the offset and address use register values from before the instruction.
//! Word/dword PUSH and POP use `50`–`5F`, `FF` /6 and `8F` /0. PUSH also accepts
//! an operand-sized immediate (`68`) or a sign-extended byte (`6A`). The stack
//! pointer uses SP or ESP according to SS.B; PUSH reads its source before decrementing it;
//! POP uses the incremented ESP to address a memory destination. POP ESP replaces
//! the pointer with the popped dword; POP SP preserves the incremented high word.
//! Segment PUSH (`06`/`0E`/`16`/`1E`/`0F A0`/`0F A8`) and POP
//! (`07`/`17`/`1F`/`0F A1`/`0F A9`) access only two selector bytes, even with a
//! dword stack adjustment, following P6-family behavior. The upper slot bytes are
//! untouched. POP resolves the selector before committing ESP and the cache;
//! POP SS uses the old SS.B for this adjustment and ends the block at cache commit.
//! Address-size and segment prefixes do not change the implicit SS stack access.
//! LES/LDS (`C4`/`C5`) and LSS/LFS/LGS (`0F B2`/`B4`/`B5`) load a GPR offset
//! and segment selector from memory. Operand size selects a word or dword offset,
//! followed by a word selector: the complete source span is four or six bytes.
//! The source uses entry addresses and caches. Full-span checks and selector
//! resolution precede both commits; word GPR writes preserve their upper half.
//! These loads preserve flags and terminate the block. The loaded offset is data;
//! later accesses check it against the newly loaded segment.
//! MOVZX (`0F B6`/`0F B7`) and MOVSX (`0F BE`/`0F BF`) read a byte/word
//! register or memory source into a dword destination, or a word with `66`.
//! They zero-extend or sign-extend from the opcode's fixed source width. The
//! word-to-word `66 0F B7`/`66 0F BF` forms copy the source unchanged.
//! CBW (`66 98`) sign-extends AL into AX; CWDE (`98`) sign-extends AX into EAX.
//! CWD (`66 99`) fills DX with the sign of AX; CDQ (`99`) fills EDX with the sign
//! of EAX. CWD/CDQ preserve the input accumulator, and word destinations preserve
//! their parent's upper half. These opcode-only forms preserve every flag.
//! LEA (`8D`) writes a ModRM/SIB effective address to a dword register, or its
//! low word with `66`. Address size independently controls offset calculation. It preserves flags,
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
//! CLC (`F8`), STC (`F9`) and CMC (`F5`) clear, set or complement CF while
//! preserving the other status flags. CLD (`FC`) and STD (`FD`) clear or set DF,
//! preserving the entire status record. These forms have no operands; `66` does
//! not change their effects. DF is a separate byte at offset 19 and is exposed
//! as `CpuState::flags.bytes.df` in host snapshots.
//! LAHF (`9F`) writes AH as SF:ZF:0:AF:0:PF:1:CF without changing flags.
//! SAHF (`9E`) copies AH bits 7/6/4/2/0 to SF/ZF/AF/PF/CF, preserving OF, DF
//! and other flags. Both use AH with or without `66` and preserve the rest of EAX.
//! MOVS, STOS, LODS, CMPS and SCAS process byte, word or dword elements with
//! address-sized SI/DI or ESI/EDI indices. DF selects increasing or decreasing indices.
//! `F3` repeats MOVS/STOS until address-sized CX/ECX reaches zero, decrementing it only after
//! each successful element. Zero count skips data access. A fault retains successful
//! elements, current indices and remaining count, with EIP at the first prefix.
//! The complete REP instruction retires once, including zero-count execution;
//! it ends a snapshot block. Repeated LODS/CMPS/SCAS and `F2` remain unsupported.
//! Relative JMP uses `EB`/`E9`; Jcc uses `70`–`7F`/`0F 80`–`0F 8F`.
//! Short displacements are signed bytes; near displacements are word/dword-sized.
//! Targets are relative to the end of the instruction. With `66`, taken targets
//! are truncated to sixteen bits even for short branches; an untaken Jcc retains
//! the full 32-bit fallthrough EIP. Near CALL (`E8` relative, `FF` /2 indirect)
//! reads its target before pushing the fallthrough pointer at operand width.
//! Indirect JMP (`FF` /4) reads an absolute target without changing ESP.
//! Near RET (`C3`) pops its target; `C2` then adds an unsigned imm16 cleanup
//! byte count to the SS.B-sized stack pointer. Only the return-pointer cell is accessed. Word targets
//! are zero-extended, and word CALL saves the low fallthrough pointer.
//! CALL and RET preserve all registers except ESP and preserve every flag.
//! JCXZ/JECXZ (`E3`) tests address-sized CX/ECX for zero without changing it. LOOP (`E2`)
//! decrements that counter and branches when nonzero; LOOPE (`E1`) also requires ZF set, LOOPNE (`E0`)
//! requires ZF clear. All preserve flags, and all use signed byte displacements.
//! Address size selects the counter independently of the operand-sized taken target.
//! Far JMP (`EA` immediate or `FF /5` memory) reads an operand-sized offset followed
//! by a word selector. The memory form checks exactly four or six bytes through the
//! old cache. It resolves a CPL3 direct code descriptor, checks the returned limit,
//! then commits CS and the target EIP together. Descriptor faults precede #GP(0)
//! for an excessive target. Registers, stack and flags are preserved. Operand size
//! determines target width; the new CS.D controls subsequent decoding defaults.
//! Far CALL (`9A` immediate, `FF /3` memory) resolves CS, checks stack capacity,
//! checks the new target and proves frame write access before saving old CS and
//! the operand-sized fallthrough offset. Far RET (`CB`, `CA imm16` cleanup) reads
//! the frame, requires RPL3, resolves CS and checks the target before committing
//! ESP and CS. Both reserve two operand-sized slots, check the complete frame
//! against SS and transfer an operand-sized offset plus two selector bytes.
//! Unused selector padding is preserved and excluded from paging, following the
//! emulator's chosen interpretation of P6 selector transfers and RET's slot check.
//! Frame fields are consecutive; SS.B wraps pointer arithmetic, including cleanup.
//! Gates, privilege transitions and real-mode transfers remain outside the subset.
//! Transfers retire once and dispatch without
//! fetching the destination instruction. Snapshot blocks end at the first control
//! transfer, REP, segment load or requested instruction limit, whichever comes first.
//! Each full memory access is checked before instruction effects. A fault
//! preserves the current instruction or string element's entry state and publishes earlier progress.
//! Address size selects 16-bit BX/BP/SI/DI or 32-bit ModRM/SIB effective addresses
//! and the width of absolute offsets, independently of data width. Offsets wrap
//! before segment translation; each access checks the complete consecutive byte span.
//! Ordinary memory operands select SS for an encoded EBP/ESP base and DS otherwise;
//! 16-bit BP addressing also selects SS; an EBP index alone does not. `26`/`2E`/`36`/`3E`/`64`/`65` override
//! that choice with ES/CS/SS/DS/FS/GS. LEA computes only an offset. String sources
//! accept overrides; destinations always use ES. Implicit stack accesses always
//! use SS. Segment permissions and the entire offset span are checked before paging.
//! A failed SS access raises stack fault with error zero; other segment failures
//! raise general protection with error zero. Linear base addition wraps at 32 bits,
//! and a valid span can cross linear zero. For a full-size expand-up segment,
//! wasm86 permits offset-span wrap; finite limits and expand-down segments reject it.
//! The returned [`CompiledModule::segment_profile`] records the entry assumptions:
//! [`compile_block_from_bytes`] defaults to [`SegmentProfile::Flat32`];
//! [`compile_block_from_bytes_with_profile`] and the interpreter accept an explicit
//! profile. Flat entries omit segment checks and base
//! reads for address defaults, statically known DS/ES/SS accesses, and CS reads.
//! Interpreter operands with an explicit segment override use complete checked
//! translation. Segmented entries check data accesses through the loaded caches.
//! The interpreter validates CS spans and fetches bytes at CS.base + EIP through
//! paging. Snapshot blocks require the caller to have validated those instruction
//! fetches, including CS permissions and limits, and do not repeat the checks.
//! The host must preserve snapshot validity through entry, direct links and execution,
//! revalidating or invalidating affected entries when relevant CS state, code bytes
//! or mappings change. An instruction that changes relied-upon assumptions ends
//! the block. Profile compatibility alone does not establish fetch validity.
//! [`DescriptorTables`] provides host-managed global/local descriptor slots and
//! protected-mode CPL3 resolution into [`StoredSegment`]. Table edits preserve
//! already-loaded caches. MOV from a segment (`8C /0`–`/5`) reads its visible selector
//! even if its cache is unusable. A word GPR destination preserves its upper half;
//! a dword GPR destination zero-extends. Memory destinations always store two bytes.
//! MOV to ES/SS/DS/FS/GS (`8E /0`, `/2`–`/5`) reads a word source, using the old cache
//! for memory, calls the host resolver, and commits the returned cache only on success. It ends
//! the block, retires once and dispatches. The resolver import and fault contract
//! are documented by [`compile_interpreter_step`]. Windows selector allocation
//! APIs, real-mode loading and interrupt/debug delivery, including SS-load inhibition,
//! are not implemented.
//! Taken near transfers check CS before publishing instruction effects; destination
//! paging belongs to the next fetch. Segmented32 requires CS.D=1, Segmented16
//! requires CS.D=0, and both handle SS.B at runtime. Flat32 requires CS.D=1, SS.B=1
//! and flat readable CS. The host must preserve compatibility until a terminal
//! segment load. Only publication and dispatch follow the cache commit; the next
//! entry must reestablish compatibility and snapshot validity. The execution owner
//! invalidates dependent entries and links when assumptions break. Far transfers validate
//! the new CS limit even when entered under Flat32.
//! CS.D sets the operand/address defaults; `66` and `67` independently select the
//! other size. Byte operands stay byte-sized. Prefixes may occur in any order.
//! Repeated `66`, `67` and `F3` preserve presence; wasm86 uses the last segment
//! override when there are several. `F2` and LOCK are outside the subset. Instructions contain at most fifteen bytes,
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
//! INC/DEC and CLC/STC/CMC publish through the same concrete format;
//! NEG uses SUB with a zero left operand.
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
//! record. Only CLD/STD change the direction flag byte, storing canonical 0 or 1
//! to DF while preserving its neighboring bytes.
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
mod exception;
mod execution;
mod flags;
mod instruction;
mod interpreter;
mod memory;
mod register;
mod runtime;
mod segment;
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

pub use block::{compile_block_from_bytes, compile_block_from_bytes_with_profile};
pub use exception::{Exception, ExceptionVector};
pub use interpreter::compile_interpreter_step;
pub use register::Gpr32;
pub use segment::{
    DescriptorTables, PrivilegeLevel, Segment, SegmentAttributes, SegmentDefaultSize,
    SegmentDescriptor, SegmentDescriptorKind, SegmentKind, SegmentProfile,
};
pub use state::{
    CpuState, FlagBytes, Registers, Segments, StoredFlags, StoredSegment, StoredStatusSource,
};

/// A WebAssembly module and the exported function that enters it.
pub struct CompiledModule {
    pub bytes: Vec<u8>,
    pub entry: String,
    /// Required segment assumptions for x86 execution entries. The host must
    /// establish compatibility before entry and invalidate dependent code and
    /// links when assumptions break. A terminal segment load may change compatibility
    /// before publication and dispatch. Snapshot instruction-fetch validity is separate.
    /// Modules that do not execute x86 instructions have no segment profile.
    pub segment_profile: Option<SegmentProfile>,
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
    /// `opcode` is the first byte after size and segment prefixes; other fields may
    /// select an unsupported form. Extended opcodes report `0F`; an unsupported
    /// form after `F3` reports `F3`.
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

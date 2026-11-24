//! x86 execution entries generated as WebAssembly.
//!
//! [`Compiler`] compiles captured instruction bytes under a [`CompilerConfig`].
//! [`compile_block_from_bytes`] uses flat 32-bit entry assumptions and host page tables.
//! [`build_interpreter`] takes entry assumptions and a memory backend. Its `run` and
//! `step` entries fetch live instructions and read CPU features at runtime.
//! Both generators share the instruction semantics.
//!
//! The instruction set covers 16/32-bit protected-mode and ordinary real-mode
//! integer execution, x87 floating point, MMX, SSE and SSE2. [`CpuFeature`] lists
//! the configurable capabilities, all enabled by default.
//! Real mode supports external interrupt entry; protected-mode interrupt delivery
//! and privilege transitions remain outside the current scope.
//!
//! [`CpuState`] exchanges backing state with the host. [`EntryAssumptions`] describes
//! the CPU state guaranteed on every entry, and [`DescriptorTables`] resolves host-managed selectors into
//! loaded caches. The shared contract for instantiating and entering generated
//! modules follows below.
//!
#![doc = include_str!("../docs/host-integration.md")]
#![doc = include_str!("../docs/physical-memory.md")]
#![forbid(unsafe_code)]

pub mod abi;
mod address;
mod alu;
mod compile;
mod decode;
mod entry;
mod exception;
mod execution;
mod flags;
mod floating;
mod instruction;
mod interpreter;
mod memory;
mod processor;
mod profiling;
pub mod real_mode;
mod register;
mod runtime;
mod segment;
mod simd;
mod ssa;
mod state;
mod x87;

#[cfg(test)]
extern crate self as relocade_x86;

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
mod support;

#[cfg(test)]
use support::step as test_step;

#[cfg(test)]
#[path = "../tests/instructions.rs"]
mod instruction_tests;

use std::fmt;

pub use compile::{
    compile_block_from_bytes, compile_block_from_bytes_with_config, BlockCompileError, BlockEntry,
    BlockSnapshot, CodeAddress, CompiledBlocks, Compiler, CompilerConfig, DecodedBlock, EntryTable,
};
pub use decode::SnapshotInstruction;
pub use entry::{EntryAssumptions, ExecutionMode, MemoryBackend, SegmentAssumptions};
pub use exception::{Exception, ExceptionCase, ExceptionValues, ExceptionVector};
pub use instruction::{CallTarget, InstructionFlow, MAX_INSTRUCTION_BYTES};
pub use interpreter::{build_interpreter, build_pc_interpreter};
pub use memory::{page_table, MmioHandlerId, PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};
pub use processor::{CpuFeature, CpuModel};
pub use profiling::{CallCount, CallProfile};
pub use register::Gpr32;
pub use segment::{
    Descriptor, DescriptorTables, PrivilegeLevel, Segment, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorInfo, SegmentDescriptorInfoValues, SegmentDescriptorKind, SegmentLimit,
};
pub use state::{
    CpuMode, CpuState, DescriptorTableRegister, ExecutionContext, ExecutionExit, ExecutionExitCase,
    ExecutionExitValues, FlagBytes, FlagsState, HostExit, HostExitCase, HostExitValues, Registers,
    SegmentRegister, Segments, SimdState, StatusSourceState, SystemSegmentRegister, X87Control,
    X87Register, X87State, X87Status,
};

/// A WebAssembly module and the exported function that enters it.
pub struct CompiledModule {
    pub bytes: Vec<u8>,
    pub entry: String,
    /// Configuration required by x86 entries. The host establishes entry assumptions
    /// and snapshot fetch validity across dispatch, direct links and interpreter handoffs.
    /// Modules that do not execute x86 instructions have no configuration.
    pub config: Option<CompilerConfig>,
}

/// A failure to construct a block, not an exception raised by guest execution.
#[derive(Debug, Eq, PartialEq)]
pub enum BlockError {
    ZeroInstructionLimit,
    /// A decoded instruction does not start at the next offset in its block.
    NoncontiguousInstruction {
        expected: u32,
        actual: u32,
    },
    /// Raw snapshot bytes require either a guaranteed or a captured CS.D.
    MissingCodeDefaultSize {
        address: u32,
    },
    /// A decoded instruction uses a different CS.D from the rest of its block.
    InconsistentCodeSize {
        address: u32,
    },
    /// An instruction that ends a block for this target precedes another instruction.
    InteriorBlockBoundary {
        address: u32,
    },
    /// Two blocks have the same start in a code view or the same export name.
    DuplicateBlock {
        address: CodeAddress,
    },
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
    UnsupportedInstruction {
        address: u32,
    },
    Compiler(relocade_compiler::BuildError),
}

impl fmt::Display for BlockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroInstructionLimit => {
                formatter.write_str("a block must contain at least one instruction")
            }
            Self::NoncontiguousInstruction { expected, actual } => {
                write!(
                    formatter,
                    "expected instruction at {expected:#x}, found {actual:#x}"
                )
            }
            Self::MissingCodeDefaultSize { address } => {
                write!(
                    formatter,
                    "decoding bytes at {address:#x} requires a guaranteed or captured CS.D"
                )
            }
            Self::InconsistentCodeSize { address } => {
                write!(
                    formatter,
                    "instruction at {address:#x} changes decode width within a block"
                )
            }
            Self::InteriorBlockBoundary { address } => {
                write!(
                    formatter,
                    "instruction at {address:#x} ends the block before its last instruction"
                )
            }
            Self::DuplicateBlock { address } => {
                write!(formatter, "duplicate block entry at {address}")
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
            Self::UnsupportedInstruction { address } => {
                write!(formatter, "unsupported instruction at {address:#x}")
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

impl From<relocade_compiler::BuildError> for BlockError {
    fn from(error: relocade_compiler::BuildError) -> Self {
        Self::Compiler(error)
    }
}

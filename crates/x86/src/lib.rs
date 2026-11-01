//! x86 execution entries generated as WebAssembly.
//!
//! [`compile_block_from_bytes`] decodes a supplied snapshot under flat 32-bit
//! assumptions. [`compile_block_from_bytes_with_profile`] selects explicit execution
//! assumptions; [`compile_interpreter`] generates runtime instruction decoding
//! through a block boundary, and [`compile_interpreter_step`] generates a
//! single-instruction entry.
//! Both frontends share instruction semantics and return a [`CompiledModule`].
//! [`Compiler`] configures both frontends with one [`ExecutionTarget`], combining
//! an execution profile and a [`CpuModel`].
//!
//! The instruction set covers 16/32-bit protected-mode and ordinary real-mode
//! integer execution, x87 floating point, MMX, SSE and SSE2. [`CpuFeature`] lists
//! the configurable capabilities, all enabled by default.
//! Real16 supports external interrupt entry; protected-mode interrupt delivery
//! and privilege transitions remain outside the current scope.
//!
//! [`CpuState`] exchanges backing state with the host. [`ExecutionProfile`] describes
//! entry assumptions, and [`DescriptorTables`] resolves host-managed selectors into
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
mod exception;
mod execution;
mod flags;
mod floating;
mod instruction;
mod memory;
mod processor;
mod profile;
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
    compile_block_from_bytes, compile_block_from_bytes_with_profile, compile_interpreter,
    compile_interpreter_step, BlockEntry, BlockSnapshot, CompiledBlocks, Compiler, EntryTable,
};
pub use exception::{Exception, ExceptionCase, ExceptionValues, ExceptionVector};
pub use memory::{page_table, PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};
pub use processor::{CpuFeature, CpuModel};
pub use profile::{ExecutionProfile, ExecutionTarget};
pub use register::Gpr32;
pub use segment::{
    Descriptor, DescriptorTables, PrivilegeLevel, Segment, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorInfo, SegmentDescriptorInfoValues, SegmentDescriptorKind, SegmentLimit,
    SegmentProfile,
};
pub use state::{
    CpuState, DescriptorTableRegister, ExecutionContext, ExecutionExit, ExecutionExitCase,
    ExecutionExitValues, FlagBytes, FlagsState, HostExit, HostExitCase, HostExitValues, Registers,
    SegmentRegister, Segments, SimdState, StatusSourceState, SystemSegmentRegister, X87Control,
    X87Register, X87State, X87Status,
};

/// A WebAssembly module and the exported function that enters it.
pub struct CompiledModule {
    pub bytes: Vec<u8>,
    pub entry: String,
    /// Required processor model and execution assumptions for x86 entries.
    /// The host must establish target compatibility before entry and use matching
    /// targets for cached blocks, direct links and interpreter fallback. A terminal
    /// segment load may change compatibility before publication and dispatch.
    /// Snapshot instruction-fetch validity is separate. Modules that do not
    /// execute x86 instructions have no execution target.
    pub execution_target: Option<ExecutionTarget>,
}

/// A failure to construct a block, not an exception raised by guest execution.
#[derive(Debug, Eq, PartialEq)]
pub enum BlockError {
    ZeroInstructionLimit,
    /// Two snapshots would export the same CS-relative entry.
    DuplicateBlock {
        address: u32,
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
    /// `opcode` is the first byte after size and segment prefixes; other fields may
    /// select an unsupported form. Extended opcodes report `0F`; an unsupported
    /// form after `F0`, `F2` or `F3` reports the selected prefix.
    UnsupportedInstruction {
        address: u32,
        opcode: u8,
    },
    Compiler(relocade_compiler::BuildError),
}

impl fmt::Display for BlockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroInstructionLimit => {
                formatter.write_str("a block must contain at least one instruction")
            }
            Self::DuplicateBlock { address } => {
                write!(formatter, "duplicate block entry at {address:#x}")
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

impl From<relocade_compiler::BuildError> for BlockError {
    fn from(error: relocade_compiler::BuildError) -> Self {
        Self::Compiler(error)
    }
}

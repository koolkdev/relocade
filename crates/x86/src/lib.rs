//! x86 user-mode execution entries generated as WebAssembly.
//!
//! [`compile_block_from_bytes`] decodes a supplied snapshot under flat 32-bit
//! assumptions. [`compile_block_from_bytes_with_profile`] selects explicit segment
//! assumptions; [`compile_interpreter_step`] generates runtime instruction decoding.
//! Both frontends share instruction semantics and return a [`CompiledModule`].
//!
//! The supported subset covers 16/32-bit protected-mode integer execution.
//! Real mode, privilege transitions, interrupt delivery, floating point and SIMD
//! are outside the current scope.
//!
//! [`CpuState`] exchanges backing state with the host. [`SegmentProfile`] describes
//! entry assumptions, and [`DescriptorTables`] resolves host-managed selectors into
//! loaded caches. The shared contract for instantiating and entering generated
//! modules follows below.
//!
#![doc = include_str!("../docs/host-integration.md")]
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
    SegmentDescriptor, SegmentDescriptorInfo, SegmentDescriptorKind, SegmentKind, SegmentLimit,
    SegmentProfile,
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
    /// form after `F2` or `F3` reports the selected repeat prefix.
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

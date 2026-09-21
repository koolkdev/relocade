//! Literal instruction checkpoints, executed individually and as one block.

mod execution;
#[cfg(test)]
mod tests;

use super::{
    cases::{
        ExpectedExit, ExpectedFlags, ExpectedState, FlagExpectation, Flags, InitialFlags,
        InitialState, MemoryExpectation, MemoryRegion, Permissions, Profiles, RegisterExpectation,
    },
    guest::Mapping,
};
use crate::flags::Flag;
use wasm86_x86::{Gpr32, Segment, StoredFlags, StoredSegment};

pub(crate) struct SequenceCase {
    name: String,
    initial: InitialState,
    checkpoints: Vec<Checkpoint>,
    trailing_code: Vec<u8>,
    trailing_instructions: u32,
    preserve_flags: bool,
    profiles: Profiles,
}

impl SequenceCase {
    pub(crate) fn new(name: impl Into<String>, flags: Flags<bool>) -> Self {
        Self::with_flags(
            name,
            InitialFlags::Logical {
                values: flags,
                stored: None,
            },
            false,
        )
    }

    /// Preserve an opaque flag record except explicitly expected direct-flag changes.
    pub(crate) fn preserving_flags(name: impl Into<String>) -> Self {
        Self::with_flags(name, InitialFlags::Opaque { stored: None }, true)
    }

    pub(crate) fn from_opaque_flags(name: impl Into<String>) -> Self {
        Self::with_flags(name, InitialFlags::Opaque { stored: None }, false)
    }

    fn with_flags(name: impl Into<String>, flags: InitialFlags, preserve_flags: bool) -> Self {
        Self {
            name: name.into(),
            initial: InitialState::new(flags),
            checkpoints: Vec::new(),
            trailing_code: Vec::new(),
            trailing_instructions: 0,
            preserve_flags,
            profiles: Profiles::All,
        }
    }

    pub(crate) fn at(mut self, origin: u32) -> Self {
        self.initial.eip = origin;
        self
    }
    pub(crate) fn instruction_count(mut self, count: u32) -> Self {
        self.initial.instruction_count = count;
        self
    }
    pub(crate) fn initial_register(mut self, register: Gpr32, value: u32) -> Self {
        self.initial.registers.push((register, value));
        self
    }
    pub(crate) fn initial_registers(mut self, registers: &[(Gpr32, u32)]) -> Self {
        self.initial.registers.extend_from_slice(registers);
        self
    }
    pub(crate) fn stored_flags(mut self, record: StoredFlags) -> Self {
        self.initial.flags.set_record(record);
        self
    }
    pub(crate) fn segmented_only(mut self) -> Self {
        self.profiles = Profiles::Segmented;
        self
    }
    pub(crate) fn segment(mut self, segment: Segment, cache: StoredSegment) -> Self {
        self.initial.segments.push((segment, cache));
        self
    }
    pub(crate) fn memory(mut self, address: u32, bytes: &[u8], permissions: Permissions) -> Self {
        self.initial.memory.push(MemoryRegion {
            address,
            bytes: bytes.to_vec(),
            permissions,
        });
        self
    }
    pub(crate) fn map_page(mut self, page: u32, frame: u32, permissions: Permissions) -> Self {
        self.initial.mappings.push(Mapping {
            page,
            frame,
            permissions,
        });
        self
    }
    pub(crate) fn backing(mut self, offset: u32, bytes: &[u8]) -> Self {
        self.initial.backing.push((offset, bytes.to_vec()));
        self
    }
    pub(crate) fn step(mut self, checkpoint: Checkpoint) -> Self {
        self.checkpoints.push(checkpoint);
        self
    }
    /// Include instructions compiled after the final expected fault or branch.
    pub(crate) fn trailing_code(mut self, bytes: &[u8], instructions: u32) -> Self {
        self.trailing_code.extend_from_slice(bytes);
        self.trailing_instructions += instructions;
        self
    }
}

pub(crate) struct Checkpoint {
    code: Vec<u8>,
    expected: ExpectedState,
}

impl Checkpoint {
    pub(crate) fn new(code: &[u8], flags: Flags<FlagExpectation>) -> Self {
        Self {
            code: code.to_vec(),
            expected: ExpectedState::new(ExpectedFlags::Logical {
                values: flags,
                preserve_record: false,
            }),
        }
    }
    /// Preserve the prior flag record except explicitly expected direct-flag changes.
    pub(crate) fn preserving_flags(code: &[u8]) -> Self {
        Self {
            code: code.to_vec(),
            expected: ExpectedState::new(ExpectedFlags::Preserved),
        }
    }
    pub(crate) fn register(self, register: Gpr32, value: u32) -> Self {
        self.expect_register(register, RegisterExpectation::Exact(value))
    }
    pub(crate) fn expect_direct_flag(mut self, flag: Flag, value: bool) -> Self {
        self.expected.expect_direct_flag(flag, value);
        self
    }
    pub(crate) fn expect_register(
        mut self,
        register: Gpr32,
        expectation: RegisterExpectation,
    ) -> Self {
        self.expected.registers.push((register, expectation));
        self
    }
    pub(crate) fn expect_memory(mut self, address: u32, bytes: &[u8]) -> Self {
        self.expected.memory.push(MemoryExpectation::Exact {
            address,
            bytes: bytes.to_vec(),
        });
        self
    }
    pub(crate) fn undefined_memory(mut self, address: u32, length: u32) -> Self {
        self.expected
            .memory
            .push(MemoryExpectation::Undefined { address, length });
        self
    }
    pub(crate) fn dispatch(mut self, target: u32) -> Self {
        self.expected.exit = ExpectedExit::Dispatch(target);
        self
    }
    pub(crate) fn fault(mut self, address: u32, error: u16) -> Self {
        self.expected.exit = ExpectedExit::PageFault { address, error };
        self
    }
    pub(crate) fn general_protection(mut self, error: u16) -> Self {
        self.expected.exit = ExpectedExit::GeneralProtection { error };
        self
    }
    pub(crate) fn stack_fault(mut self, error: u16) -> Self {
        self.expected.exit = ExpectedExit::StackFault { error };
        self
    }
    pub(crate) fn divide_error(mut self) -> Self {
        self.expected.exit = ExpectedExit::DivideError;
        self
    }

    pub(crate) fn bound_range_exceeded(mut self) -> Self {
        self.expected.exit = ExpectedExit::BoundRangeExceeded;
        self
    }
}

pub(crate) use execution::check as check_sequences;

macro_rules! test_sequences {
    ($group:ident, $cases:expr $(,)?) => {
        $crate::support::execution::test_frontends!(
            $group,
            $cases,
            $crate::support::sequences::check_sequences
        );
    };
}
pub(crate) use test_sequences;

//! Literal input/output cases for one guest instruction.

mod execution;
pub(super) mod expectations;
mod initial;
pub(super) mod observation;

#[cfg(test)]
mod tests;

use super::guest::Mapping;
use crate::flags::Flag;
use wasm86_x86::{
    CpuState, Gpr32, Segment, SegmentDefaultSize, SegmentProfile, StoredFlags, StoredSegment,
};

pub(crate) use super::guest::Permissions;

pub(crate) struct InstructionCase {
    pub(super) name: String,
    pub(super) code: Vec<u8>,
    pub(super) initial: InitialState,
    pub(super) expected: ExpectedState,
    pub(super) profiles: Profiles,
}

pub(super) enum Profiles {
    All,
    Segmented,
}

impl Profiles {
    pub(super) fn for_cpu(&self, cpu: &CpuState) -> impl Iterator<Item = SegmentProfile> {
        let segmented = match cpu.segments.cs.attributes.default_size() {
            SegmentDefaultSize::Bits16 => SegmentProfile::Segmented16,
            SegmentDefaultSize::Bits32 => SegmentProfile::Segmented32,
        };
        [
            Some(segmented),
            matches!(self, Self::All).then_some(SegmentProfile::Flat32),
        ]
        .into_iter()
        .flatten()
    }
}

impl InstructionCase {
    /// Start at 0x1000, retire one instruction, and dispatch past its encoding.
    /// Unlisted registers and memory must preserve their initial values.
    pub(crate) fn new(
        name: impl Into<String>,
        code: &[u8],
        initial_flags: Flags<bool>,
        expected_flags: Flags<FlagExpectation>,
    ) -> Self {
        Self::with_flags(
            name,
            code,
            InitialFlags::Logical {
                values: initial_flags,
                stored: None,
            },
            ExpectedFlags::Logical {
                values: expected_flags,
                preserve_record: false,
            },
        )
    }

    /// Preserve an opaque flag record except explicitly expected direct-flag changes.
    /// Instructions that inspect flags should state logical values with `new`.
    pub(crate) fn preserving_flags(name: impl Into<String>, code: &[u8]) -> Self {
        Self::with_flags(
            name,
            code,
            InitialFlags::Opaque { stored: None },
            ExpectedFlags::Preserved,
        )
    }

    /// Replace an opaque incoming record with the stated logical flags.
    /// No flag may use `Preserved`, since the initial values are unspecified.
    pub(crate) fn replacing_flags(
        name: impl Into<String>,
        code: &[u8],
        expected: Flags<FlagExpectation>,
    ) -> Self {
        Self::with_flags(
            name,
            code,
            InitialFlags::Opaque { stored: None },
            ExpectedFlags::Logical {
                values: expected,
                preserve_record: false,
            },
        )
    }

    fn with_flags(
        name: impl Into<String>,
        code: &[u8],
        initial: InitialFlags,
        expected: ExpectedFlags,
    ) -> Self {
        Self {
            name: name.into(),
            code: code.to_vec(),
            initial: InitialState::new(initial),
            expected: ExpectedState::new(expected),
            profiles: Profiles::All,
        }
    }

    /// The case requires segment state outside the flat profile.
    pub(crate) fn segmented_only(mut self) -> Self {
        self.profiles = Profiles::Segmented;
        self
    }

    pub(crate) fn segment(mut self, segment: Segment, cache: StoredSegment) -> Self {
        self.initial.segments.push((segment, cache));
        self
    }

    pub(crate) fn register(self, register: Gpr32, input: u32, output: u32) -> Self {
        self.initial_register(register, input)
            .expect_register(register, RegisterExpectation::Exact(output))
    }

    pub(crate) fn initial_register(mut self, register: Gpr32, input: u32) -> Self {
        self.initial.registers.push((register, input));
        self
    }

    pub(crate) fn initial_registers(mut self, registers: &[(Gpr32, u32)]) -> Self {
        self.initial.registers.extend_from_slice(registers);
        self
    }

    pub(crate) fn instruction_count(mut self, count: u32) -> Self {
        self.initial.instruction_count = count;
        self
    }

    /// Set the incoming stored flag record. A case with logical initial flags
    /// also verifies that the record represents those stated values.
    pub(crate) fn stored_flags(mut self, flags: StoredFlags) -> Self {
        self.initial.flags.set_record(flags);
        self
    }

    /// Preserve the incoming flag record except explicitly expected direct-flag changes.
    pub(crate) fn preserve_flag_record(mut self) -> Self {
        if let ExpectedFlags::Logical {
            preserve_record, ..
        } = &mut self.expected.flags
        {
            *preserve_record = true;
        }
        self
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

    pub(crate) fn expect_memory(mut self, address: u32, bytes: &[u8]) -> Self {
        self.expected.memory.push(MemoryExpectation::Exact {
            address,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub(crate) fn at(mut self, origin: u32) -> Self {
        self.initial.eip = origin;
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

    pub(crate) fn divide_error(mut self) -> Self {
        self.expected.exit = ExpectedExit::DivideError;
        self
    }

    pub(crate) fn bound_range_exceeded(mut self) -> Self {
        self.expected.exit = ExpectedExit::BoundRangeExceeded;
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

    pub(super) fn expected_eip(&self) -> u32 {
        match self.expected.exit {
            ExpectedExit::Fallthrough => self.initial.eip.wrapping_add(self.code.len() as u32),
            ExpectedExit::Dispatch(target) => target,
            ExpectedExit::DivideError
            | ExpectedExit::BoundRangeExceeded
            | ExpectedExit::GeneralProtection { .. }
            | ExpectedExit::StackFault { .. }
            | ExpectedExit::PageFault { .. } => self.initial.eip,
        }
    }

    pub(super) fn expected_retired(&self) -> u32 {
        match self.expected.exit {
            ExpectedExit::Fallthrough | ExpectedExit::Dispatch(_) => 1,
            ExpectedExit::DivideError
            | ExpectedExit::BoundRangeExceeded
            | ExpectedExit::GeneralProtection { .. }
            | ExpectedExit::StackFault { .. }
            | ExpectedExit::PageFault { .. } => 0,
        }
    }
}

pub(super) struct InitialState {
    pub(super) flags: InitialFlags,
    pub(super) eip: u32,
    pub(super) instruction_count: u32,
    pub(super) registers: Vec<(Gpr32, u32)>,
    pub(super) segments: Vec<(Segment, StoredSegment)>,
    pub(super) memory: Vec<MemoryRegion>,
    pub(super) mappings: Vec<Mapping>,
    pub(super) backing: Vec<(u32, Vec<u8>)>,
}

#[derive(Clone)]
pub(super) struct ExpectedState {
    pub(super) flags: ExpectedFlags,
    pub(super) direct_flags: Vec<(Flag, bool)>,
    pub(super) registers: Vec<(Gpr32, RegisterExpectation)>,
    pub(super) memory: Vec<MemoryExpectation>,
    pub(super) exit: ExpectedExit,
}

impl ExpectedState {
    pub(super) fn expect_direct_flag(&mut self, flag: Flag, value: bool) {
        assert!(
            matches!(flag, Flag::TF | Flag::DF | Flag::NT | Flag::AC | Flag::ID),
            "direct flag expectations require TF, DF, NT, AC or ID; use logical expectations for status flags"
        );
        if let Some((_, expected)) = self.direct_flags.iter_mut().find(|(name, _)| *name == flag) {
            *expected = value;
        } else {
            self.direct_flags.push((flag, value));
        }
    }

    pub(super) fn new(flags: ExpectedFlags) -> Self {
        Self {
            registers: Vec::new(),
            memory: Vec::new(),
            exit: ExpectedExit::Fallthrough,
            flags,
            direct_flags: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum InitialFlags {
    Logical {
        values: Flags<bool>,
        stored: Option<StoredFlags>,
    },
    Opaque {
        stored: Option<StoredFlags>,
    },
}

impl InitialFlags {
    pub(super) fn set_record(&mut self, record: StoredFlags) {
        let (Self::Logical { stored, .. } | Self::Opaque { stored }) = self;
        *stored = Some(record);
    }
    pub(super) fn logical(self) -> Option<Flags<bool>> {
        match self {
            Self::Logical { values, .. } => Some(values),
            Self::Opaque { .. } => None,
        }
    }
    pub(super) fn record(self) -> Option<StoredFlags> {
        match self {
            Self::Logical { stored, .. } | Self::Opaque { stored } => stored,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ExpectedFlags {
    Logical {
        values: Flags<FlagExpectation>,
        preserve_record: bool,
    },
    Preserved,
}

impl ExpectedFlags {
    pub(super) fn preserves_record(self) -> bool {
        matches!(
            self,
            Self::Preserved
                | Self::Logical {
                    preserve_record: true,
                    ..
                }
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Flags<T> {
    pub(crate) cf: T,
    pub(crate) pf: T,
    pub(crate) af: T,
    pub(crate) zf: T,
    pub(crate) sf: T,
    pub(crate) of: T,
}

impl<T: Copy> Flags<T> {
    pub(crate) const fn all(value: T) -> Self {
        Self {
            cf: value,
            pf: value,
            af: value,
            zf: value,
            sf: value,
            of: value,
        }
    }

    pub(super) fn values(self) -> [T; 6] {
        [self.cf, self.pf, self.af, self.zf, self.sf, self.of]
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FlagExpectation {
    Set,
    Clear,
    Preserved,
    Undefined,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RegisterExpectation {
    Exact(u32),
    DefinedBits { value: u32, mask: u32 },
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ExpectedExit {
    Fallthrough,
    Dispatch(u32),
    DivideError,
    BoundRangeExceeded,
    GeneralProtection { error: u16 },
    StackFault { error: u16 },
    PageFault { address: u32, error: u16 },
}

pub(super) struct MemoryRegion {
    pub(super) address: u32,
    pub(super) bytes: Vec<u8>,
    pub(super) permissions: Permissions,
}

#[derive(Clone)]
pub(super) enum MemoryExpectation {
    Exact { address: u32, bytes: Vec<u8> },
    Undefined { address: u32, length: u32 },
}

pub(crate) use execution::check as check_cases;

/// Register one case group in the normal test run and the explicit V8 lane.
macro_rules! test_cases {
    ($group:ident, $cases:expr $(,)?) => {
        $crate::support::execution::test_frontends!(
            $group,
            $cases,
            $crate::support::cases::check_cases
        );
    };
}

pub(crate) use test_cases;

//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_profile::{
    EntryAssumptions, ExecutionMode, ExecutionProfile, MemoryBackend, SegmentAssumptions,
};
use std::sync::Arc;

use crate::{
    state::{access::cpu_location, cr0, cr4, StatePattern},
    CpuMode, CpuModel, CpuState, FlagBytes, PrivilegeLevel,
};

/// Bits consumed by mode and privilege predicates. Other supported-state
/// restrictions remain in CPU admission until their transitions are implemented.
pub(crate) fn control_assumptions(mode: ExecutionMode) -> Vec<StatePattern> {
    macro_rules! require_bits {
        ($($field:ident).+, $mask:expr, $expected:expr) => {
            StatePattern::bits(
                cpu_location!($($field).+),
                u64::from($mask),
                u64::from($expected),
            )
        };
    }
    let protected = mode == ExecutionMode::ProtectedUser;
    let mut requirements = vec![
        require_bits!(
            cr0,
            cr0::PROTECTION_ENABLE,
            if protected { cr0::PROTECTION_ENABLE } else { 0 }
        ),
        require_bits!(flags.bytes.vm, FlagBytes::BOOLEAN_MASK, 0u8),
        require_bits!(cr4, cr4::TIMESTAMP_DISABLE, 0u32),
    ];
    if protected {
        requirements.push(require_bits!(
            segments.cs.selector,
            u16::from(PrivilegeLevel::MASK),
            PrivilegeLevel::Ring3 as u16
        ));
    }
    requirements
}

/// CPU field values the execution owner establishes on every entry.
/// Builders may fold reads from these facts before emitting any guard.
#[derive(Clone)]
pub(crate) struct EntryGuarantees {
    pub(crate) entry: EntryAssumptions,
    fields: Arc<[StatePattern]>,
}

impl EntryGuarantees {
    pub(crate) fn from_assumptions(entry: EntryAssumptions) -> Self {
        let mut fields = control_assumptions(entry.mode);
        for segment in crate::Segment::ALL {
            fields.extend(crate::segment::profile::assumptions(entry, segment));
        }
        Self {
            entry,
            fields: fields.into(),
        }
    }

    pub(crate) fn patterns(&self) -> &[StatePattern] {
        &self.fields
    }

    #[cfg(test)]
    pub(crate) fn for_profile(profile: ExecutionProfile) -> Self {
        Self::from_assumptions(profile.into())
    }
}

/// The processor model and execution assumptions required by a compiled entry.
///
/// Use the complete value in cache and direct-link keys. Blocks and their fallback
/// interpreters must agree on this target, including disabled capabilities.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionTarget {
    pub profile: ExecutionProfile,
    /// Memory routing and imports required by instruction fetch and data accesses.
    pub memory_backend: MemoryBackend,
    pub cpu_model: CpuModel,
    /// Whether the execution owner may arm instruction budgets. When false,
    /// snapshot blocks omit their budget checks and the live budget must stay zero.
    pub instruction_budgets: bool,
}

impl ExecutionTarget {
    /// Architectural facts required by this target.
    pub fn assumptions(self) -> EntryAssumptions {
        self.profile.into()
    }

    /// Check an entry against the installed target and architectural CPU state.
    ///
    /// The CPU image supplies the mode; the target retains generated-code policy
    /// and specialization requirements. Code bytes, fetch permissions and mappings require
    /// separate validation as described by the entry-validity contract.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && cpu.is_compatible_with(self.assumptions())
    }
}

impl CpuState {
    /// Checks the entry guarantees and the supported control state. Control bits
    /// outside the initialized baseline can be stored but cannot execute yet.
    /// This check does not reload segments or authorize mode transitions.
    pub fn is_compatible_with(&self, entry: impl Into<EntryAssumptions>) -> bool {
        let entry = entry.into();
        let mode = match entry.mode {
            ExecutionMode::Real => CpuMode::Real,
            ExecutionMode::ProtectedUser => CpuMode::Protected,
        };
        self.mode() == mode
            && self.flags.bytes.vm & FlagBytes::BOOLEAN_MASK == 0
            // ET and NE are set; EM, TS, paging and other unimplemented controls are clear.
            && self.cr0 & !cr0::PROTECTION_ENABLE == (cr0::EXTENSION_TYPE | cr0::NUMERIC_ERROR)
            // OSFXSR and OSXMMEXCPT are set; TSD and other controls are clear.
            && self.cr4 == (cr4::OS_FXSR | cr4::OS_XMM_EXCEPTIONS)
            && self.segments.is_compatible_with(entry)
    }
}

#[cfg(test)]
mod tests;

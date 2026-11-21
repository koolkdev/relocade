//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_profile::{ExecutionProfile, MemoryBackend};
use std::sync::Arc;

use crate::{
    state::{access::cpu_location, cr0, cr4, StatePattern},
    CpuMode, CpuModel, CpuState, FlagBytes, PrivilegeLevel, SegmentDefaultSize,
};

/// The hosted user environment exposes IF=1 and IOPL=0 independently of
/// the saved EFLAGS bytes. The PC uses the architectural fields.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum InterruptFlagPolicy {
    Architectural,
    HostedUser,
}

impl InterruptFlagPolicy {
    pub(crate) fn for_profile(profile: ExecutionProfile) -> Self {
        match profile {
            ExecutionProfile::Real16 => Self::Architectural,
            ExecutionProfile::Protected(_) => Self::HostedUser,
        }
    }

    pub(crate) fn fixed_iopl(self) -> Option<PrivilegeLevel> {
        match self {
            Self::Architectural => None,
            Self::HostedUser => Some(PrivilegeLevel::Ring0),
        }
    }
}

/// Bits consumed by mode and privilege predicates. Other supported-state
/// restrictions remain in CPU admission until their transitions are implemented.
pub(crate) fn control_assumptions(profile: ExecutionProfile) -> Vec<StatePattern> {
    macro_rules! require_bits {
        ($($field:ident).+, $mask:expr, $expected:expr) => {
            StatePattern::bits(
                cpu_location!($($field).+),
                u64::from($mask),
                u64::from($expected),
            )
        };
    }
    let protected = matches!(profile, ExecutionProfile::Protected(_));
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

/// CPU field values the execution owner establishes on every entry to this profile.
/// Builders may fold reads from these facts before emitting any guard.
#[derive(Clone)]
pub(crate) struct EntryGuarantees {
    pub(crate) interrupt_flags: InterruptFlagPolicy,
    fields: Arc<[StatePattern]>,
}

impl EntryGuarantees {
    pub(crate) fn for_profile(profile: ExecutionProfile) -> Self {
        Self::new(profile, Some(SegmentDefaultSize::for_profile(profile)))
    }

    /// Protected entries may leave CS.D live. Canonical real-mode caches always fix it.
    pub(crate) fn new(
        profile: ExecutionProfile,
        protected_code_default: Option<SegmentDefaultSize>,
    ) -> Self {
        let mut fields = control_assumptions(profile);
        for segment in crate::Segment::ALL {
            fields.extend(crate::segment::profile::assumptions(
                profile,
                segment,
                protected_code_default,
            ));
        }
        Self {
            interrupt_flags: InterruptFlagPolicy::for_profile(profile),
            fields: fields.into(),
        }
    }

    pub(crate) fn patterns(&self) -> &[StatePattern] {
        &self.fields
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
    /// Check an entry against the installed target and architectural CPU state.
    ///
    /// The CPU image supplies the mode; the target retains generated-code policy
    /// and specialization requirements. Code bytes, fetch permissions and mappings require
    /// separate validation as described by the entry-validity contract.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && cpu.is_compatible_with(self.profile)
    }
}

impl CpuState {
    /// Admission to the existing fixed-profile execution model. Control bits
    /// outside the initialized baseline can be stored but cannot execute yet.
    /// This check does not reload segments or authorize mode transitions.
    pub fn is_compatible_with(&self, profile: impl Into<ExecutionProfile>) -> bool {
        let profile = profile.into();
        let mode = match profile {
            ExecutionProfile::Real16 => CpuMode::Real,
            ExecutionProfile::Protected(_) => CpuMode::Protected,
        };
        self.mode() == mode
            && self.flags.bytes.vm & FlagBytes::BOOLEAN_MASK == 0
            // ET and NE are set; EM, TS, paging and other unimplemented controls are clear.
            && self.cr0 & !cr0::PROTECTION_ENABLE == (cr0::EXTENSION_TYPE | cr0::NUMERIC_ERROR)
            // OSFXSR and OSXMMEXCPT are set; TSD and other controls are clear.
            && self.cr4 == (cr4::OS_FXSR | cr4::OS_XMM_EXCEPTIONS)
            && self.segments.is_compatible_with(profile)
    }
}

#[cfg(test)]
mod tests;

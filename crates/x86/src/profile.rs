//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_profile::{ExecutionProfile, MemoryBackend};
use std::{mem::offset_of, sync::Arc};

use crate::{
    state::{access::cpu_location, cr0, cr4, StatePattern},
    CpuMode, CpuModel, CpuState, ExecutionContext, FlagBytes, FlagsState, PrivilegeLevel,
    SegmentDefaultSize,
};

macro_rules! require_bits {
    ($($field:ident).+, $mask:expr, $expected:expr) => {
        StatePattern::bits(
            cpu_location!($($field).+),
            u64::from($mask),
            u64::from($expected),
        )
    };
}

/// Bits consumed by mode and privilege predicates. Other supported-state
/// restrictions remain in CPU admission until their transitions are implemented.
fn control_assumptions(target: ExecutionTarget) -> Vec<StatePattern> {
    let protected = matches!(target.profile, ExecutionProfile::Protected(_));
    let mut requirements = vec![
        require_bits!(
            cr0,
            cr0::PROTECTION_ENABLE,
            if protected { cr0::PROTECTION_ENABLE } else { 0 }
        ),
        require_bits!(cr4, cr4::TIMESTAMP_DISABLE, 0u32),
    ];
    if protected {
        requirements.push(require_bits!(
            segments.cs.selector,
            u16::from(PrivilegeLevel::MASK),
            target.privilege_level() as u16
        ));
    }
    requirements.extend(target.flag_assumptions());
    requirements
}

/// CPU field values the execution owner establishes on every entry to this profile.
/// Builders may fold reads from these facts before emitting any guard.
#[derive(Clone)]
pub(crate) struct EntryGuarantees(Arc<[StatePattern]>);

impl EntryGuarantees {
    pub(crate) fn for_profile(profile: ExecutionProfile) -> Self {
        Self::for_target(ExecutionTarget::new(
            profile,
            MemoryBackend::default_for(profile),
        ))
    }

    pub(crate) fn for_target(target: ExecutionTarget) -> Self {
        Self::new(
            target,
            Some(SegmentDefaultSize::for_profile(target.profile)),
        )
    }

    /// Protected entries may leave CS.D live. Canonical real-mode caches always fix it.
    pub(crate) fn new(
        target: ExecutionTarget,
        protected_code_default: Option<SegmentDefaultSize>,
    ) -> Self {
        let mut fields = control_assumptions(target);
        for segment in crate::Segment::ALL {
            fields.extend(crate::segment::profile::assumptions(
                target.profile,
                segment,
                protected_code_default,
            ));
        }
        Self(fields.into())
    }

    pub(crate) fn patterns(&self) -> &[StatePattern] {
        &self.0
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
    pub(crate) fn new(profile: ExecutionProfile, memory_backend: MemoryBackend) -> Self {
        Self {
            profile,
            memory_backend,
            cpu_model: CpuModel::default(),
            instruction_budgets: true,
        }
    }

    pub(crate) fn privilege_level(self) -> PrivilegeLevel {
        match self.profile {
            ExecutionProfile::Real16 => PrivilegeLevel::Ring0,
            ExecutionProfile::Protected(_) => PrivilegeLevel::Ring3,
        }
    }

    /// The hosted user configuration fixes IOPL. PC configurations leave it live.
    pub(crate) fn guaranteed_iopl(self) -> Option<PrivilegeLevel> {
        match (self.profile, self.memory_backend) {
            (ExecutionProfile::Protected(_), MemoryBackend::HostPageTable) => {
                Some(PrivilegeLevel::Ring0)
            }
            _ => None,
        }
    }

    fn flag_assumptions(self) -> impl Iterator<Item = StatePattern> {
        let hosted = self.guaranteed_iopl().map(|iopl| {
            [
                require_bits!(flags.bytes.if_, FlagBytes::BOOLEAN_MASK, 1u8),
                require_bits!(flags.bytes.iopl, PrivilegeLevel::MASK, iopl as u8),
            ]
        });
        std::iter::once(require_bits!(flags.bytes.vm, FlagBytes::BOOLEAN_MASK, 0u8))
            .chain(hosted.into_iter().flatten())
    }

    fn admits_flag_record(self, record: &[u8], offset: usize) -> bool {
        self.flag_assumptions()
            .filter(|pattern| pattern.within_record(offset, record.len()))
            .all(|pattern| pattern.matches_record(record, offset))
    }

    /// Checks a flag record before restoring it or replacing the represented EFLAGS.
    pub fn admits_flags(self, flags: &FlagsState) -> bool {
        self.admits_flag_record(&flags.to_bytes(), offset_of!(ExecutionContext, cpu.flags))
    }

    /// Checks an isolated IF write without reading the other live flags.
    pub fn admits_interrupts_enabled(self, enabled: bool) -> bool {
        self.admits_flag_record(
            &[u8::from(enabled)],
            offset_of!(ExecutionContext, cpu.flags.bytes.if_),
        )
    }

    /// Check an entry against the installed target and architectural CPU state.
    ///
    /// The CPU image supplies the mode; the target retains generated-code policy
    /// and specialization requirements. Code bytes, fetch permissions and mappings require
    /// separate validation as described by the entry-validity contract.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && self.admits_cpu(cpu)
    }

    /// Checks whether the architectural CPU state satisfies this target's entry guarantees.
    /// Control bits outside the supported baseline can be stored but cannot execute yet.
    pub fn admits_cpu(self, cpu: &CpuState) -> bool {
        let mode = match self.profile {
            ExecutionProfile::Real16 => CpuMode::Real,
            ExecutionProfile::Protected(_) => CpuMode::Protected,
        };
        cpu.mode() == mode
            && self.admits_flags(&cpu.flags)
            // ET and NE are set; EM, TS, paging and other unimplemented controls are clear.
            && cpu.cr0 & !cr0::PROTECTION_ENABLE == (cr0::EXTENSION_TYPE | cr0::NUMERIC_ERROR)
            // OSFXSR and OSXMMEXCPT are set; TSD and other controls are clear.
            && cpu.cr4 == (cr4::OS_FXSR | cr4::OS_XMM_EXCEPTIONS)
            && cpu.segments.is_compatible_with(self.profile)
    }
}

impl CpuState {
    /// Checks admission for the default execution target of this profile.
    pub fn is_compatible_with(&self, profile: impl Into<ExecutionProfile>) -> bool {
        let profile = profile.into();
        ExecutionTarget::new(profile, MemoryBackend::default_for(profile)).admits_cpu(self)
    }
}

#[cfg(test)]
mod tests;

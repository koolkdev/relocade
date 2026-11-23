//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_profile::{ExecutionProfile, MemoryBackend};
use std::{mem::offset_of, sync::Arc};

use crate::{
    state::{access::cpu_location, cr0, cr4, StatePattern},
    CpuModel, CpuState, ExecutionContext, FlagBytes, PrivilegeLevel, SegmentDefaultSize,
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

/// Control bits established by the runtime and available to generated code on entry.
pub(crate) fn control_assumptions(
    target: ExecutionTarget,
) -> impl Iterator<Item = StatePattern> + Clone {
    let protected = matches!(target.profile, ExecutionProfile::Protected(_));
    let hosted = protected && target.memory_backend == MemoryBackend::HostPageTable;
    [
        Some(require_bits!(
            cr0,
            cr0::PROTECTION_ENABLE,
            if protected { cr0::PROTECTION_ENABLE } else { 0 }
        )),
        Some(require_bits!(cr4, cr4::TIMESTAMP_DISABLE, 0u32)),
        Some(require_bits!(flags.bytes.vm, FlagBytes::BOOLEAN_MASK, 0u8)),
        protected.then(|| {
            require_bits!(
                segments.cs.selector,
                u16::from(PrivilegeLevel::MASK),
                PrivilegeLevel::Ring3 as u16
            )
        }),
        hosted.then(|| require_bits!(flags.bytes.if_, FlagBytes::BOOLEAN_MASK, 1u8)),
        hosted.then(|| require_bits!(flags.bytes.iopl, PrivilegeLevel::MASK, 0u8)),
    ]
    .into_iter()
    .flatten()
}

/// CPU field values the execution owner establishes on every entry to this profile.
/// Builders may fold reads from these facts before emitting any guard.
#[derive(Clone)]
pub(crate) struct EntryGuarantees(Arc<[StatePattern]>);

impl EntryGuarantees {
    pub(crate) fn for_profile(profile: ExecutionProfile) -> Self {
        Self::for_target(profile)
    }

    pub(crate) fn for_target(target: impl Into<ExecutionTarget>) -> Self {
        let target = target.into();
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
        let mut fields: Vec<_> = control_assumptions(target).collect();
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

    /// Checks the constrained control bits touched by a host write to the CPU image.
    /// The offset is relative to `CpuState`. Bits outside the write keep their live values.
    pub fn matches_control_write(self, offset: usize, bytes: &[u8]) -> bool {
        if offset > CpuState::BYTE_LEN || bytes.len() > CpuState::BYTE_LEN - offset {
            return false;
        }
        let offset = offset_of!(ExecutionContext, cpu) + offset;
        control_assumptions(self).all(|field| field.matches_record(bytes, offset))
    }

    /// Check an entry against the installed target and architectural CPU state.
    ///
    /// The CPU image supplies the mode; the target retains generated-code policy
    /// and specialization requirements. Code bytes, fetch permissions and mappings require
    /// separate validation as described by the entry-validity contract.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && cpu.is_compatible_with(self)
    }
}

impl<P: Into<ExecutionProfile>> From<P> for ExecutionTarget {
    fn from(profile: P) -> Self {
        let profile = profile.into();
        Self::new(profile, MemoryBackend::default_for(profile))
    }
}

impl CpuState {
    /// Checks the control state and segment caches before running this CPU image.
    /// Passing a profile selects its default memory backend.
    pub fn is_compatible_with(&self, target: impl Into<ExecutionTarget>) -> bool {
        let target = target.into();
        target.matches_control_write(0, &self.to_bytes())
            // ET and NE are set; EM, TS, paging and other unimplemented controls are clear.
            && self.cr0 & !cr0::PROTECTION_ENABLE == (cr0::EXTENSION_TYPE | cr0::NUMERIC_ERROR)
            // OSFXSR and OSXMMEXCPT are set; TSD and other controls are clear.
            && self.cr4 == (cr4::OS_FXSR | cr4::OS_XMM_EXCEPTIONS)
            && self.segments.is_compatible_with(target.profile)
    }
}

#[cfg(test)]
mod tests;

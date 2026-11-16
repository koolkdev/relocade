//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_profile::ExecutionProfile;

use crate::{
    state::{cr0, cr4, StateAssumption},
    CpuMode, CpuModel, CpuState, FlagBytes, PrivilegeLevel,
};
use std::{mem::offset_of, sync::Arc};

/// Entry guarantees maintained by the current profile's admission and transitions.
pub(crate) fn state_assumptions(profile: ExecutionProfile) -> Arc<[StateAssumption]> {
    let mut assumptions = control_assumptions(profile);
    for segment in crate::Segment::ALL {
        assumptions.extend(crate::segment::profile::assumptions(profile, segment));
    }
    assumptions.into()
}

/// Bits consumed by mode and privilege predicates. Other supported-state
/// restrictions remain in CPU admission until their transitions are implemented.
fn control_assumptions(profile: ExecutionProfile) -> Vec<StateAssumption> {
    macro_rules! require_bits {
        ($($field:ident).+, $mask:expr, $expected:expr) => {
            StateAssumption::bits(
                offset_of!(CpuState, $($field).+),
                |cpu: &CpuState| &cpu.$($field).+,
                $mask,
                $expected,
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
        require_bits!(flags.bytes.vm, FlagBytes::BOOLEAN_MASK, 0),
        require_bits!(cr4, cr4::TIMESTAMP_DISABLE, 0),
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

/// The processor model and execution assumptions required by a compiled entry.
///
/// Use the complete value in cache and direct-link keys. Blocks and their fallback
/// interpreters must agree on this target, including disabled capabilities.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionTarget {
    pub profile: ExecutionProfile,
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

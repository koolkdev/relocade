//! Execution mode and assumptions fixed when an entry is compiled.

mod requirements;
pub use requirements::{EntryFact, EntryRequirements, SegmentFact};

pub use relocade_x86_profile::ExecutionProfile;

use crate::{CpuMode, CpuModel, CpuState};

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
            && self.flags.bytes.vm & 1 == 0
            // ET and NE are set; EM, TS, paging and other unimplemented controls are clear.
            && self.cr0 & !1 == 0x30
            // OSFXSR and OSXMMEXCPT are set; TSD and other controls are clear.
            && self.cr4 == 0x600
            && self.segments.is_compatible_with(profile)
    }
}

#[cfg(test)]
mod tests;

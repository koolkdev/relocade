//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_profile::ExecutionProfile;

use crate::{CpuModel, Segments};

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
    /// Check an entry against the host's active target and loaded segment caches.
    ///
    /// The host must supply its actual execution mode; segment caches alone cannot
    /// establish that mode. Code bytes, fetch permissions and mappings require
    /// separate validation as described by the entry-validity contract.
    pub fn is_compatible_with(self, active: Self, segments: &Segments) -> bool {
        self == active && segments.is_compatible_with(self.profile)
    }
}

#[cfg(test)]
mod tests;

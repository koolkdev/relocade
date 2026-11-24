//! Configuration for snapshot compilation and compiled artifact compatibility.

use crate::{CpuModel, CpuState, EntryAssumptions, MemoryBackend};

/// The entry facts and code-generation settings required by a compiled artifact.
/// Include the complete configuration in cache and direct-link keys.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CompilerConfig {
    pub entry: EntryAssumptions,
    /// Memory routing and imports used for instruction fetch and data accesses.
    pub memory_backend: MemoryBackend,
    pub cpu_model: CpuModel,
    /// Disabled budgets require the owner to keep the live budget zero.
    pub instruction_budgets: bool,
}

impl CompilerConfig {
    /// Uses the default processor model and enables instruction budgets.
    pub const fn new(entry: EntryAssumptions, memory_backend: MemoryBackend) -> Self {
        Self {
            entry,
            memory_backend,
            cpu_model: CpuModel::DEFAULT,
            instruction_budgets: true,
        }
    }

    /// Checks constrained control bits touched by a host write relative to `CpuState`.
    pub fn matches_control_write(self, offset: usize, bytes: &[u8]) -> bool {
        crate::entry::matches_control_write(self.entry, offset, bytes)
    }

    /// Checks the artifact configuration and current CPU image. Code bytes and
    /// instruction-fetch mappings require separate validation.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && cpu.is_compatible_with(self.entry)
    }
}

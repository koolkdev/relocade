//! Entry guarantees and code-generation settings shared by a module's callers.

use crate::{CpuModel, CpuState, EntryAssumptions, MemoryBackend};

/// The assumptions guaranteed on every entry and the environment used by generated code.
/// Include all fields in keys for compiled snapshots and direct links.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CompilerConfig {
    pub entry: EntryAssumptions,
    /// Routing and imports used for instruction fetch and data access.
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

    /// Checks the active configuration and CPU image before executing a compiled artifact.
    /// Code bytes, instruction-fetch permissions and mappings are validated separately.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && cpu.is_compatible_with(self.entry)
    }
}

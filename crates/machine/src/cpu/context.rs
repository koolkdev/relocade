use crate::{CpuConfig, CpuStorage, CpuView, MachineConfig, MemoryView};
use relocade_x86::ExecutionTarget;

/// Scoped access to one stopped CPU and its guest memory. Other CPUs may share
/// the memory. Both views read and write live storage directly.
pub struct CpuContext<'a> {
    cpu: &'a CpuConfig,
    config: &'a MachineConfig,
    storage: &'a mut dyn CpuStorage,
}

impl<'a> CpuContext<'a> {
    /// Binds access to initialized storage. Its images and capacities must match
    /// `config`; CPU state must satisfy the configured execution target. Host
    /// CPU edits enforce compatibility before publishing changes.
    pub fn new(
        cpu: &'a CpuConfig,
        config: &'a MachineConfig,
        storage: &'a mut dyn CpuStorage,
    ) -> Self {
        Self {
            cpu,
            config,
            storage,
        }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.cpu.execution_target()
    }

    pub fn cpu(&mut self) -> CpuView<'_> {
        CpuView::new(self.cpu, self.storage)
    }

    pub fn memory(&mut self) -> MemoryView<'_> {
        MemoryView::new(self.config, self.storage)
    }
}

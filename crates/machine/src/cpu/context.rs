use crate::{CpuConfig, CpuStorage, CpuView, MachineConfig, MemoryView};
use relocade_x86::ExecutionTarget;

/// Scoped access to one stopped CPU and its guest memory. Other CPUs may share
/// the memory. Both views read and write live storage directly.
/// The context retains immutable configuration while borrowing the live storage.
pub struct CpuContext<'a> {
    target: ExecutionTarget,
    config: MachineConfig,
    storage: &'a mut dyn CpuStorage,
}

impl<'a> CpuContext<'a> {
    /// Binds access to initialized storage. Its images and capacities must match
    /// `config`; CPU state must satisfy the configured execution target. Host
    /// CPU edits enforce compatibility before publishing changes.
    pub fn new(cpu: &CpuConfig, config: &MachineConfig, storage: &'a mut dyn CpuStorage) -> Self {
        Self::with_configuration(cpu.execution_target(), config.clone(), storage)
    }

    pub(crate) fn with_configuration(
        target: ExecutionTarget,
        config: MachineConfig,
        storage: &'a mut dyn CpuStorage,
    ) -> Self {
        Self {
            target,
            config,
            storage,
        }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.target
    }

    pub fn cpu(&mut self) -> CpuView<'_> {
        CpuView::for_target(self.target, self.storage)
    }

    pub fn memory(&mut self) -> MemoryView<'_> {
        MemoryView::new(&self.config, self.storage)
    }
}

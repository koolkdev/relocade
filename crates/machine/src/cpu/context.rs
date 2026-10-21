use crate::{CpuConfig, CpuStorage, CpuView, MachineConfig, Memory};
use relocade_x86::ExecutionTarget;

/// Scoped access to one stopped CPU and its address space. Other CPUs may share
/// the memory. Both views read and write live storage directly.
pub struct CpuContext<'a, S: CpuStorage + ?Sized = dyn CpuStorage + 'a> {
    cpu: &'a CpuConfig,
    config: &'a MachineConfig,
    storage: &'a mut S,
}

impl<'a, S: CpuStorage + ?Sized> CpuContext<'a, S> {
    /// Binds access to initialized storage. Its images and capacities must match
    /// `config`; CPU state must satisfy the configured execution target. Host
    /// CPU edits enforce compatibility before publishing changes.
    pub fn new(cpu: &'a CpuConfig, config: &'a MachineConfig, storage: &'a mut S) -> Self {
        Self {
            cpu,
            config,
            storage,
        }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.cpu.execution_target()
    }

    pub fn cpu(&mut self) -> CpuView<'_, S> {
        CpuView::new(self.cpu, self.storage)
    }

    pub fn memory(&mut self) -> Memory<'_, S> {
        Memory::new(self.config, self.storage)
    }
}

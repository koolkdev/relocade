use crate::{CpuStorage, CpuView, MachineConfig, Memory};
use relocade_x86::ExecutionTarget;

/// Scoped access to the stopped CPU and guest memory. Both views read and write
/// execution storage directly.
pub struct CpuContext<'a, S: CpuStorage + ?Sized = dyn CpuStorage + 'a> {
    config: &'a MachineConfig,
    storage: &'a mut S,
}

impl<'a, S: CpuStorage + ?Sized> CpuContext<'a, S> {
    /// Binds access to initialized storage. The CPU must be compatible with
    /// `config`'s execution target; the map and backing capacity must also match.
    pub fn new(config: &'a MachineConfig, storage: &'a mut S) -> Self {
        Self { config, storage }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.config.execution_target()
    }

    pub fn cpu(&mut self) -> CpuView<'_, S> {
        CpuView::new(self.config, self.storage)
    }

    pub fn memory(&mut self) -> Memory<'_, S> {
        Memory::new(self.config, self.storage)
    }
}

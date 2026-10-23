use crate::{CpuConfig, CpuView, MachineConfig, MemoryView};
use relocade_wasm_runtime::MemoryAccess;
use relocade_x86::ExecutionTarget;

/// Scoped access to one stopped CPU and its guest memory. Other CPUs may share
/// the memory. Both views read and write live storage directly.
pub struct CpuContext<'a> {
    cpu: &'a CpuConfig,
    config: &'a MachineConfig,
    context: MemoryAccess<'a>,
    guest: MemoryAccess<'a>,
}

impl<'a> CpuContext<'a> {
    /// Binds an initialized `ExecutionContext` image and guest backing. Their
    /// contents and capacities must match the configurations; CPU state must
    /// satisfy the execution target. CPU edits check compatibility before writing.
    #[inline]
    pub fn new(
        cpu: &'a CpuConfig,
        config: &'a MachineConfig,
        context: MemoryAccess<'a>,
        guest: MemoryAccess<'a>,
    ) -> Self {
        Self {
            cpu,
            config,
            context,
            guest,
        }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.cpu.execution_target()
    }

    #[inline]
    pub fn cpu(&mut self) -> CpuView<'_> {
        CpuView::new(self.cpu, self.context.reborrow())
    }

    #[inline]
    pub fn memory(&mut self) -> MemoryView<'_> {
        MemoryView::new(self.config, self.guest.reborrow())
    }
}

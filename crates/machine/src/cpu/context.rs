use crate::{CpuView, MemoryView};
use relocade_x86::ExecutionTarget;

/// Borrowed views of one stopped CPU and its shared guest memory.
/// Register edits and memory transfers apply directly to their respective owners.
pub struct CpuContext<'a> {
    cpu: CpuView<'a>,
    memory: MemoryView<'a>,
}

impl<'a> CpuContext<'a> {
    pub fn new(cpu: CpuView<'a>, memory: MemoryView<'a>) -> Self {
        Self { cpu, memory }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.cpu.execution_target()
    }

    pub fn cpu(&mut self) -> CpuView<'_> {
        self.cpu.reborrow()
    }

    pub fn memory(&mut self) -> MemoryView<'_> {
        self.memory.reborrow()
    }

    pub fn descriptors(&mut self) -> crate::Descriptors<'_> {
        crate::Descriptors::new(self.memory.descriptors.reborrow(), self.cpu.reborrow())
    }
}

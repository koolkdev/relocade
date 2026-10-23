use super::RuntimeMemory;
use crate::{CpuStorage, MemoryStorage};
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::ExecutionContext;
use std::{borrow::Borrow, mem::offset_of};

/// Accesses a CPU and its bound memories through an owning backend or a
/// borrowed callback context. Reads and writes reach live memory immediately.
pub(crate) struct CpuAccess<A: WasmMemoryAccess, M = RuntimeMemory<<A as WasmMemoryAccess>::Memory>>
{
    pub backend: A,
    pub bindings: M,
}

impl<A: WasmMemoryAccess, M: Borrow<RuntimeMemory<A::Memory>>> CpuAccess<A, M> {
    pub fn set_instruction_target(&mut self, target: u64) {
        self.backend.write_memory(
            &self.bindings.borrow().context,
            offset_of!(ExecutionContext, instruction_target) as u32,
            &target.to_le_bytes(),
        );
    }
}

impl<A: WasmMemoryAccess, M: Borrow<RuntimeMemory<A::Memory>>> CpuStorage for CpuAccess<A, M> {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]) {
        self.backend.read_memory(
            &self.bindings.borrow().context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]) {
        self.backend.write_memory(
            &self.bindings.borrow().context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }
}

impl<A: WasmMemoryAccess, M: Borrow<RuntimeMemory<A::Memory>>> MemoryStorage for CpuAccess<A, M> {
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        self.backend
            .read_memory(&self.bindings.borrow().guest, offset, bytes);
    }
    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.backend
            .write_memory(&self.bindings.borrow().guest, offset, bytes);
    }
}

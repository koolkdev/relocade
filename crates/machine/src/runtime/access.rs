use super::RuntimeMemory;
use crate::{CpuOffset, MemoryStorage, ScalarAccess};
use relocade_wasm_runtime::{MemoryValue, WasmMemoryAccess};
use relocade_x86::ExecutionContext;
use std::mem::offset_of;

/// Accesses a CPU and its bound memories through an owning backend or a
/// borrowed callback context. Reads and writes reach live memory immediately.
pub(crate) struct CpuAccess<B: WasmMemoryAccess> {
    pub backend: B,
    pub bindings: RuntimeMemory<B::Memory>,
}

impl<B: WasmMemoryAccess> CpuAccess<B> {
    pub fn set_instruction_target(&mut self, target: u64) {
        self.backend.store_memory(
            &self.bindings.context,
            offset_of!(ExecutionContext, instruction_target) as u32,
            target,
        );
    }
}

/// Resolves the two address spaces without a runtime memory selector.
trait RuntimeAddress {
    fn resolve<M>(self, bindings: &RuntimeMemory<M>) -> (&M, u32);
}

impl RuntimeAddress for CpuOffset {
    fn resolve<M>(self, bindings: &RuntimeMemory<M>) -> (&M, u32) {
        (
            &bindings.context,
            offset_of!(ExecutionContext, cpu) as u32 + self.0,
        )
    }
}

impl RuntimeAddress for u32 {
    fn resolve<M>(self, bindings: &RuntimeMemory<M>) -> (&M, u32) {
        (&bindings.guest, self)
    }
}

impl<B: WasmMemoryAccess, A: RuntimeAddress, V: MemoryValue> ScalarAccess<A, V> for CpuAccess<B> {
    fn load(&self, address: A) -> V {
        let (memory, offset) = address.resolve(&self.bindings);
        self.backend.load_memory(memory, offset)
    }

    fn store(&mut self, address: A, value: V) {
        let (memory, offset) = address.resolve(&self.bindings);
        self.backend.store_memory(memory, offset, value);
    }
}

impl<B: WasmMemoryAccess, A: RuntimeAddress> MemoryStorage<A> for CpuAccess<B> {
    fn read(&self, address: A, bytes: &mut [u8]) {
        let (memory, offset) = address.resolve(&self.bindings);
        self.backend.read_memory(memory, offset, bytes);
    }

    fn write(&mut self, address: A, bytes: &[u8]) {
        let (memory, offset) = address.resolve(&self.bindings);
        self.backend.write_memory(memory, offset, bytes);
    }
}

use crate::{CpuBindings, CpuStorage, MemoryStorage};
use relocade_wasm_runtime::WasmMemoryAccess;

/// Borrows a backend context and one CPU's memory bindings.
/// Used by synchronous and deferred host handlers while guest execution is stopped.
pub(crate) struct BackendStorage<'a, A: WasmMemoryAccess> {
    pub backend: &'a mut A,
    bindings: &'a CpuBindings<A::Memory>,
}

impl<'a, A: WasmMemoryAccess> BackendStorage<'a, A> {
    pub fn new(backend: &'a mut A, bindings: &'a CpuBindings<A::Memory>) -> Self {
        Self { backend, bindings }
    }
}

impl<A: WasmMemoryAccess> MemoryStorage for BackendStorage<'_, A> {
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        self.bindings.read_backing(self.backend, offset, bytes);
    }
    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.bindings.write_backing(self.backend, offset, bytes);
    }
}

impl<A: WasmMemoryAccess> CpuStorage for BackendStorage<'_, A> {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]) {
        self.bindings.read_cpu(self.backend, offset, bytes);
    }
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]) {
        self.bindings.write_cpu(self.backend, offset, bytes);
    }
}

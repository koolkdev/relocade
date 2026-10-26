use super::Memory;
use crate::{MemoryAccess, WasmContext, WasmMemoryAccess};
use wasmtime::{AsContextMut, StoreContextMut};

/// A callback's exclusive access to its store and memories.
pub struct Context<'a, T: 'static>(pub(super) StoreContextMut<'a, T>);

impl<T: 'static> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        self.0.data()
    }

    fn state_mut(&mut self) -> &mut T {
        self.0.data_mut()
    }
}

impl<T: 'static> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        memory.read(&self.0, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        memory.write(&mut self.0, offset, bytes);
    }

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access(self.0.as_context_mut())
    }
}

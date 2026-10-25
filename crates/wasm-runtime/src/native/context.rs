use super::Memory;
use crate::{MemoryAccess, WasmContext, WasmMemoryAccess};
use wasmtime::{AsContextMut, StoreContextMut};

/// Scoped exclusive access to a store and its memories.
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

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access(self.0.as_context_mut())
    }
}

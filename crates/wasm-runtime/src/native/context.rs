use super::Memory;
use crate::{MemoryAccess, StateAccess, WasmContext, WasmMemoryAccess};
use std::ops::{Deref, DerefMut};
use wasmtime::{AsContextMut, StoreContextMut};

/// A callback's exclusive access to its store and memories.
pub struct Context<'a, T: 'static>(pub(super) StoreContextMut<'a, T>);

impl<T: 'static> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> impl Deref<Target = T> {
        self.0.data()
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        self.0.data_mut()
    }

    fn memory_and_state<'a>(
        &'a mut self,
        memory: &'a Memory,
    ) -> (MemoryAccess<'a>, StateAccess<'a, T>) {
        memory.access_and_state(&mut self.0)
    }
}

impl<T: 'static> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access(self.0.as_context_mut())
    }
}

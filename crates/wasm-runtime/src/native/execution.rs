use super::Memory;
use crate::execution::{ScopedContext, ScopedData};
use crate::state::StoreState;
use crate::{
    ExecutionData, MemoryAccess, StateAccess, WasmCallback, WasmContext, WasmMemoryAccess,
};
use std::ops::{Deref, DerefMut};
use wasmtime::{AsContextMut, StoreContextMut};

/// Scoped access to the store and its memories, inside or outside a callback.
pub struct Context<'a, T: 'static>(pub(super) StoreContextMut<'a, StoreState<T>>);

impl<T: 'static> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> impl Deref<Target = T> {
        &self.0.data().state
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        &mut self.0.data_mut().state
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

impl<T: 'static> ScopedContext for Context<'_, T> {
    fn scoped_data(&mut self) -> &mut ScopedData {
        &mut self.0.data_mut().execution_data
    }
}

impl<T: 'static> WasmCallback for Context<'_, T> {
    fn with_execution_data<D: ExecutionData, R>(
        &mut self,
        callback: impl for<'a> FnOnce(&'a mut Self, &'a D) -> R,
    ) -> R {
        crate::execution::with_data(self, callback)
    }
}

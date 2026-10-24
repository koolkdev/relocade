use super::Memory;
use crate::{MemoryAccess, StateAccess, WasmContext, WasmMemoryAccess};
use std::{
    cell::RefMut,
    ops::{Deref, DerefMut},
};

/// A callback's exclusive borrow of its retained host state.
pub struct Context<'a, T>(pub(super) RefMut<'a, T>);

impl<T: 'static> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> impl Deref<Target = T> {
        &*self.0
    }

    fn state_mut(&mut self) -> impl DerefMut<Target = T> {
        &mut *self.0
    }

    fn memory_and_state<'a>(
        &'a mut self,
        memory: &'a Memory,
    ) -> (MemoryAccess<'a>, StateAccess<'a, T>) {
        (memory.access(), (&mut *self.0).into())
    }
}

impl<T> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access()
    }
}

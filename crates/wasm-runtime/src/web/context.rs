use super::{Memory, Store};
use crate::{MemoryAccess, WasmContext, WasmMemoryAccess};
use std::cell::RefMut;

/// Scoped exclusive access to retained host state.
pub struct Context<'a, T>(pub(super) RefMut<'a, Store<T>>);

impl<T: 'static> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        &self.0.state
    }

    fn state_mut(&mut self) -> &mut T {
        &mut self.0.state
    }
}

impl<T> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        self.0.memory(memory).access()
    }
}

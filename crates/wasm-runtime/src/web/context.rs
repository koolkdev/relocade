use super::{Memory, Store};
use crate::{MemoryAccess, WasmContext, WasmMemoryAccess};
use std::{
    cell::{RefCell, RefMut},
    rc::Rc,
};

/// Scoped access to browser functions and tables, independent of backend state.
/// Resources belong to the JavaScript thread and retain their own engine handles.
pub struct TableContext<'a, T>(std::marker::PhantomData<&'a mut Rc<T>>);

impl<T> TableContext<'_, T> {
    pub(super) fn new() -> Self {
        Self(std::marker::PhantomData)
    }

    /// Borrows this scope for a nested synchronous operation.
    pub fn reborrow(&mut self) -> TableContext<'_, T> {
        TableContext::new()
    }
}

/// Scoped exclusive access to retained host state, memories and tables.
pub struct Context<'a, T> {
    pub(super) store: RefMut<'a, Store<T>>,
    pub(super) owner: &'a Rc<RefCell<Store<T>>>,
}

impl<T: 'static> WasmContext for Context<'_, T> {
    type State = T;

    fn state(&self) -> &T {
        &self.store.state
    }

    fn state_mut(&mut self) -> &mut T {
        &mut self.store.state
    }

    fn table_context(&mut self) -> TableContext<'_, T> {
        TableContext::new()
    }
}

impl<T> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        self.store.memory(memory).access()
    }
}

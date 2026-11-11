use super::{Memory, StoreData};
use crate::{MemoryAccess, WasmContext, WasmMemoryAccess};
use wasmtime::{AsContextMut, StoreContextMut};

/// Scoped exclusive access to a store, its memories, functions and tables.
pub struct Context<'a>(pub(super) StoreContextMut<'a, StoreData>);

impl WasmContext for Context<'_> {
    fn reborrow(&mut self) -> Context<'_> {
        Context(self.0.as_context_mut())
    }
}

impl WasmMemoryAccess for Context<'_> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.access(self.0.as_context_mut())
    }
}

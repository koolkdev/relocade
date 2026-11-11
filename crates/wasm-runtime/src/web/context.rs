use super::{Memory, WasmRuntime};
use crate::{MemoryAccess, WasmContext, WasmMemoryAccess};
use std::marker::PhantomData;

/// Scoped engine access on the current JavaScript thread. Resource handles own
/// their allocations, so creating or reborrowing a context needs no state lookup.
pub struct Context<'a>(PhantomData<&'a mut WasmRuntime>);

impl Context<'_> {
    pub(super) fn new() -> Self {
        Self(PhantomData)
    }
}

impl WasmContext for Context<'_> {
    fn reborrow(&mut self) -> Context<'_> {
        Context::new()
    }
}

impl WasmMemoryAccess for Context<'_> {
    type Memory = Memory;

    fn memory<'a>(&'a mut self, memory: &'a Memory) -> MemoryAccess<'a> {
        memory.0.access()
    }
}

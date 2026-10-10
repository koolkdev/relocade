//! Engine-module installation independent of its producer or scheduling.

use super::{CodeRange, Runtime, Ticket};
use wasmtime::{Module, TypedFunc};

/// An engine-compiled module and its generated entry export. It must use the
/// runtime ABI and profile, have no start function or imported-memory
/// initialization, and match the code protected by its registration.
/// Establishing artifact identity belongs to the loader.
/// Generation and engine compilation happen off the execution thread.
#[derive(Clone)]
pub struct CompiledEntry {
    pub module: Module,
    pub entry: String,
}

impl<T: 'static> Runtime<T> {
    /// Protects complete guest code dependencies without scheduling generation.
    /// Call after matching an artifact to the current guest image/context, before
    /// any asynchronous loading or compilation that can overlap guest writes.
    pub fn register_code(&mut self, eip: u32, ranges: &[CodeRange]) -> Option<Ticket> {
        self.memory.register(&mut self.store, eip, ranges)
    }

    /// Releases an abandoned registration, or invalidates an installed entry.
    pub fn cancel_code(&mut self, ticket: Ticket) {
        self.memory.cancel(&mut self.store, ticket);
        self.blocks.remove(&ticket);
    }

    /// Instantiates and publishes an already compiled entry at this execution
    /// boundary. Worker completions and prepared modules use the same operation.
    /// Returns false for stale or already installed tickets. Failure releases
    /// this registration while preserving any previously installed block.
    pub fn install(&mut self, ticket: Ticket, compiled: CompiledEntry) -> wasmtime::Result<bool> {
        self.memory.enter(&mut self.store);
        if !self.memory.is_pending(&self.store, ticket) {
            return Ok(false);
        }
        let entry = match self.instantiate(compiled) {
            Ok(entry) => entry,
            Err(error) => {
                self.cancel_code(ticket);
                return Err(error);
            }
        };
        assert!(self.memory.install(&mut self.store, ticket));
        self.blocks.insert(ticket, entry);
        Ok(true)
    }

    pub(super) fn instantiate(
        &mut self,
        compiled: CompiledEntry,
    ) -> wasmtime::Result<TypedFunc<(), i64>> {
        let instance = self.linker.instantiate(&mut self.store, &compiled.module)?;
        instance.get_typed_func(&mut self.store, &compiled.entry)
    }
}

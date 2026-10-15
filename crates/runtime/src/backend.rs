//! Host-engine integration for the generated x86 execution ABI.
//!
//! Implementations own their module/instance handles and callback lifetimes.
//! Instruction semantics belong to x86, address routing and environment services
//! to machine, and execution policy to runtime. Futures and environments need not
//! be `Send`.

use crate::Result;
use std::future::Future;
use wasm86_machine::Environment;
use wasm86_x86::{CompiledModule, CpuState, EntryTable};

pub use wasm86_machine::{GuestMemory, HostCallback, Imports};

/// Compiles and instantiates generated Wasm modules on one host engine.
pub trait Backend {
    type Module;
    type Instance<D: Environment + 'static>: Instance<D, Module = Self::Module>;

    fn compile(&self, module: &CompiledModule) -> impl Future<Output = Result<Self::Module>>;

    /// Creates private CPU, guest, map, budget and dispatch memories plus a
    /// shared function table. Install the dispatcher in slot zero and interpreter
    /// in slot one. Bind environment imports to the supplied state. All
    /// memories start zeroed; the runtime publishes maps before execution.
    fn instantiate<D: Environment + 'static>(
        &self,
        module: &Self::Module,
        dispatcher: &Self::Module,
        imports: Imports<D>,
    ) -> impl Future<Output = Result<Self::Instance<D>>>;
}

/// Cooperative work allowance and observation policy for one Wasm entry.
#[derive(Clone, Copy, Debug)]
pub struct EntryRequest {
    /// A scheduling target; execution may overshoot by at most 255 units.
    pub work: u32,
    /// Return at the next published dispatch boundary for a host observer.
    pub observe: bool,
}

impl EntryRequest {
    pub fn slice(work: u32) -> Self {
        Self {
            work,
            observe: false,
        }
    }
    pub fn observed(work: u32) -> Self {
        Self {
            work,
            observe: true,
        }
    }

    /// The shared budget ABI is a signed little-endian remaining-work balance.
    pub fn to_bytes(self) -> [u8; 8] {
        i64::from(self.work).to_le_bytes()
    }
}

/// Published progress from one generated execution entry. `completed` is the
/// wrapping CPU retirement-counter delta, independent of dispatch callbacks.
#[derive(Clone, Copy, Debug)]
pub struct EntryResult {
    pub word: u64,
    pub remaining_work: i64,
    pub completed: u64,
}

/// A callable dispatcher, installed modules and their private machine state.
pub trait Instance<D: Environment> {
    type Module;

    /// Instantiates another module against this machine's existing memories and
    /// function table. Bind fixed callbacks as host.entry_0, host.entry_1, etc.
    /// Install exports consecutively from first_slot before publishing mappings.
    /// Retain callbacks while any installed function can reach them.
    fn install_module(
        &mut self,
        module: &Self::Module,
        first_slot: u32,
        exports: &[String],
        callbacks: Vec<HostCallback<D>>,
    ) -> impl Future<Output = Result<()>>;

    /// Supplies a work allowance in `executionBudget`, enters the prepared export
    /// and reads published progress. Do not borrow import state across entry:
    /// guest execution can invoke it.
    fn enter(&mut self, request: EntryRequest) -> Result<EntryResult>;
    /// Publishes execution destinations between entries, growing their memory
    /// first. Preserve the continuation and observation prefix when replacing a table.
    fn set_entries(&mut self, entries: &EntryTable) -> Result<()>;
    /// Abandons a suspended callable so the next entry starts a fresh address lookup.
    fn restart_entry(&mut self);
    fn cpu(&self) -> CpuState;
    fn set_cpu(&mut self, cpu: &CpuState);

    fn inspect<R>(&self, inspect: impl FnOnce(&Imports<D>) -> R) -> R;

    /// Exposes import state and guest backing outside guest execution. Any map
    /// publication requested here must complete before returning. Device callbacks
    /// must provide the same guarantee before returning to generated Wasm.
    fn with_memory<R>(
        &mut self,
        access: impl FnOnce(&mut Imports<D>, &mut dyn GuestMemory) -> R,
    ) -> R;

    /// Ensures backing has at least this many bytes, preserving previous contents
    /// and zeroing newly allocated bytes. This operation never shrinks memory.
    fn grow_guest(&mut self, bytes: u64) -> Result<()>;
}

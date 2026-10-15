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

pub use wasm86_machine::{GuestMemory, Imports};

/// Compiles and instantiates generated Wasm modules on one host engine.
pub trait Backend {
    type Module;
    type Instance<D: Environment + 'static>: Instance<D, Module = Self::Module>;

    fn compile(&self, module: &CompiledModule) -> impl Future<Output = Result<Self::Module>>;

    /// Creates private execution-context, guest, map and dispatch memories plus a
    /// shared function table. Install the dispatcher in slot zero and interpreter
    /// in slot one. Bind environment imports to the supplied state. The
    /// execution backing starts with `ExecutionContext::default()`; other memories
    /// start zeroed. The runtime publishes CPU state and maps before execution.
    fn instantiate<D: Environment + 'static>(
        &self,
        module: &Self::Module,
        dispatcher: &Self::Module,
        imports: Imports<D>,
    ) -> impl Future<Output = Result<Self::Instance<D>>>;
}

/// Instruction target and observation policy for one Wasm entry.
#[derive(Clone, Copy, Debug)]
pub struct EntryRequest {
    /// Absolute retirement target; `u64::MAX` is practically unbounded.
    /// Budget ordering assumes the instruction counter does not wrap.
    pub instruction_target: u64,
    /// Return at the next dispatch boundary for a host observer.
    pub observe: bool,
}

/// A callable dispatcher, installed modules and their private machine state.
pub trait Instance<D: Environment> {
    type Module;

    /// Instantiates another module against this machine's existing memories and
    /// function table. Install exports consecutively from first_slot before
    /// publishing address mappings.
    fn install_module(
        &mut self,
        module: &Self::Module,
        first_slot: u32,
        exports: &[String],
    ) -> impl Future<Output = Result<()>>;

    /// Publishes the instruction target in execution-context backing and returns
    /// the Wasm exit word. Do not borrow import state across entry: guest execution
    /// can invoke it.
    fn enter(&mut self, request: EntryRequest) -> Result<u64>;
    /// Publishes execution destinations between entries, growing their memory
    /// first. Preserve the observation prefix when replacing a table.
    fn set_entries(&mut self, entries: &EntryTable) -> Result<()>;
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

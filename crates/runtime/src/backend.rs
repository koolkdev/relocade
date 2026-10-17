//! Host-engine integration for the generated x86 execution ABI.
//!
//! Implementations own their module/instance handles and callback lifetimes.
//! Instruction semantics belong to x86, address routing and environment services
//! to machine, and execution policy to runtime. Futures and environments need not
//! be `Send`.

use crate::Result;
use std::future::Future;
use wasm86_machine::Environment;
use wasm86_x86::{CompiledModule, CpuState};

pub use wasm86_machine::{GuestMemory, Imports};

/// Compiles and instantiates generated Wasm modules on one host engine.
pub trait Backend {
    type Module;
    type Instance<D: Environment + 'static>: Instance<D>;

    fn compile(&self, module: &CompiledModule) -> impl Future<Output = Result<Self::Module>>;

    /// Creates private execution-context, guest, map and dispatch memories plus a
    /// shared function table. Leave slot zero empty and install the interpreter
    /// in slot one. Initialize an empty x86 EntryTable and bind environment
    /// imports to the supplied state. Execution backing starts with
    /// `ExecutionContext::default()`; guest and map backing start zeroed.
    /// Allocate enough guest backing for the supplied map at instantiation.
    /// The runtime publishes CPU state and maps before execution.
    fn instantiate<D: Environment + 'static>(
        &self,
        module: &Self::Module,
        dispatcher: &Self::Module,
        imports: Imports<D>,
    ) -> impl Future<Output = Result<Self::Instance<D>>>;
}

/// A callable dispatcher, interpreter and their private machine state.
pub trait Instance<D: Environment> {
    /// Publishes the instruction target in execution-context backing and returns
    /// the Wasm exit word. Do not borrow import state across entry: guest execution
    /// can invoke it.
    fn run(&mut self, instruction_target: u64) -> Result<u64>;
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
}

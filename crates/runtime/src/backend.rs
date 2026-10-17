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
    /// shared function table. Leave slot zero empty and install the interpreter
    /// in slot one. Initialize an empty x86 EntryTable and bind environment
    /// imports to the supplied state. Execution backing starts with
    /// `ExecutionContext::default()`; guest and map backing start zeroed.
    /// Allocate guest backing from `Imports::memory_size`, rounded up to 64-KiB
    /// Wasm pages with a minimum of one page.
    /// The runtime publishes CPU state and maps before execution.
    fn instantiate<D: Environment + 'static>(
        &self,
        module: &Self::Module,
        dispatcher: &Self::Module,
        imports: Imports<D>,
    ) -> impl Future<Output = Result<Self::Instance<D>>>;
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
    fn run(&mut self, instruction_target: u64) -> Result<u64>;
    /// Publishes execution destinations between entries, growing their memory
    /// first.
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
}

/// Encodes the existing x86 descriptor import ABI for engine adapters.
#[allow(clippy::type_complexity)]
pub fn resolve_segment<E: Environment>(
    imports: &mut Imports<E>,
    segment: u32,
    selector: u32,
) -> (u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) {
    use wasm86_x86::{Exception, Segment};
    match imports
        .environment_mut()
        .resolve_segment(Segment::ALL[segment as usize], selector as u16)
    {
        Ok(cache) => (
            0,
            0,
            cache.base,
            cache.limit,
            u32::from(cache.selector),
            u32::from(cache.usable),
            u32::from(cache.readable),
            u32::from(cache.writable),
            u32::from(cache.executable),
            u32::from(cache.expand_down),
            u32::from(cache.big),
        ),
        Err(exception) => {
            let (vector, code) = match exception {
                Exception::GeneralProtection { error_code } => (13, error_code),
                Exception::StackFault { error_code } => (12, error_code),
                Exception::SegmentNotPresent { error_code } => (11, error_code),
                _ => panic!("segment resolution must return a segment fault"),
            };
            (vector, code, 0, 0, 0, 0, 0, 0, 0, 0, 0)
        }
    }
}

/// Encodes a descriptor query using the same order on native and web engines.
pub fn query_segment_descriptor<E: Environment>(
    imports: &mut Imports<E>,
    selector: u32,
) -> (u32, u32, u32, u32, u32) {
    let info = imports
        .environment_mut()
        .query_segment_descriptor(selector as u16);
    (
        u32::from(info.visible),
        u32::from(info.readable),
        u32::from(info.writable),
        info.access_rights,
        info.limit,
    )
}

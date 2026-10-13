//! Host-engine integration for the generated x86 execution ABI.
//!
//! Implementations own their module/instance handles and callback lifetimes.
//! Guest instruction semantics, physical routing and execution policy remain in
//! the x86 crate and runtime. Futures and device state need not be `Send`.

use crate::{Devices, Result};
use std::future::Future;
use wasm86_x86::{CompiledModule, CpuState, EntryTable, PhysicalMemoryMap};

pub use crate::imports::Imports;

/// Compiles and instantiates generated Wasm modules on one host engine.
pub trait Backend {
    type Module;
    type Instance<D: Devices + 'static>: Instance<D, Module = Self::Module>;

    fn compile(&self, module: &CompiledModule) -> impl Future<Output = Result<Self::Module>>;

    /// Creates private CPU, guest, map, budget and dispatch memories plus a
    /// shared function table. Install the dispatcher in slot zero and interpreter
    /// in slot one. Bind physical/device imports to the supplied state. All
    /// memories start zeroed; the runtime publishes maps before execution.
    fn instantiate<D: Devices + 'static>(
        &self,
        module: &Self::Module,
        dispatcher: &Self::Module,
        imports: Imports<D>,
    ) -> impl Future<Output = Result<Self::Instance<D>>>;
}

/// Limits and observation policy for one entry into the Wasm execution loop.
#[derive(Clone, Copy, Debug)]
pub struct EntryRequest {
    pub work: u32,
    /// Zero permits any retirement count within the work allowance.
    pub instructions: u32,
    /// Return at the next published dispatch boundary for a host observer.
    pub observe: bool,
}

impl EntryRequest {
    pub fn slice(work: u32) -> Self {
        Self {
            work,
            instructions: 0,
            observe: false,
        }
    }
    pub fn step() -> Self {
        Self {
            work: u32::MAX,
            instructions: 1,
            observe: false,
        }
    }
    pub fn observed(work: u32) -> Self {
        Self {
            work,
            instructions: 0,
            observe: true,
        }
    }

    /// The shared budget ABI contains work, retirement limit and starting counter.
    pub fn to_bytes(self, starting_count: u32) -> [u8; 12] {
        let mut bytes = [0; 12];
        bytes[..4].copy_from_slice(&self.work.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.instructions.to_le_bytes());
        bytes[8..].copy_from_slice(&starting_count.to_le_bytes());
        bytes
    }
}

/// Published progress from one generated execution entry. `completed` is the
/// wrapping CPU retirement-counter delta, independent of dispatch callbacks.
#[derive(Clone, Copy, Debug)]
pub struct EntryResult {
    pub word: u64,
    pub remaining_work: u32,
    pub completed: u32,
}

/// A callable dispatcher, installed modules and their private machine state.
pub trait Instance<D: Devices> {
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

    /// Supplies a work allowance in `executionBudget`, enters the prepared export
    /// and reads published progress. Do not borrow import state across entry:
    /// guest execution can invoke it. A dispatch can represent partial REP work.
    fn enter(&mut self, request: EntryRequest) -> Result<EntryResult>;
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

/// Scoped backing access for the host. Ranges passed to `read` and `write` must
/// fit `byte_len`; the runtime validates user transfers before invoking them.
/// Implementations must keep access coherent with the generated Wasm memories.
pub trait GuestMemory {
    fn byte_len(&self) -> u64;
    fn read(&self, offset: u32, bytes: &mut [u8]);
    fn write(&mut self, offset: u32, bytes: &[u8]);

    /// Publishes the owner's serialized physical table before this host access or
    /// device callback returns. An adapter may buffer it until its memory borrow
    /// ends, but the next guest transfer must observe the new routing.
    fn publish_map(&mut self, bytes: &[u8; PhysicalMemoryMap::BYTE_LEN]);
}

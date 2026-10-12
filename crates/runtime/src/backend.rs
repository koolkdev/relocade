//! Host-engine integration for the generated x86 execution ABI.
//!
//! Implementations own their module/instance handles and callback lifetimes.
//! Guest instruction semantics, physical routing and execution policy remain in
//! the x86 crate and runtime. Futures and device state need not be `Send`.

use crate::{Devices, Result};
use std::future::Future;
use wasm86_x86::{CompiledModule, CpuState, PhysicalMemoryMap};

pub use crate::imports::Imports;

/// Compiles and instantiates generated Wasm modules on one host engine.
pub trait Backend {
    type Module;
    type Instance<D: Devices + 'static>: Instance<D>;

    fn compile(&self, module: &CompiledModule) -> impl Future<Output = Result<Self::Module>>;

    /// Creates private CPU, guest and physical-map memories, each at least one
    /// Wasm page. Bind the supplied imports without entering guest execution.
    /// The runtime initializes CPU state, backing size and mapping before entry.
    fn instantiate<D: Devices + 'static>(
        &self,
        module: &Self::Module,
        imports: Imports<D>,
    ) -> impl Future<Output = Result<Self::Instance<D>>>;
}

/// One executable entry and its private memories/callback state.
pub trait Instance<D: Devices> {
    /// Enters the prepared export and returns its unmodified ABI word. Do not
    /// borrow import state across this call: guest execution can invoke it.
    fn enter(&mut self) -> Result<u64>;
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

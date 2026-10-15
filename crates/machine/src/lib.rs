//! Guest CPU configuration, address spaces, and execution-environment services.
//!
//! A machine describes the guest independently of the Wasm engine that executes it.
//! The active CPU target is explicit: an operating-system environment does not
//! imply a fixed x86 execution mode. Backends provide scoped backing access while
//! this crate owns address translation and environment callbacks.
#![forbid(unsafe_code)]

mod context;
mod environment;
mod host;
mod imports;
mod memory;

pub use context::{ConfigurationError, CpuContext};
pub use environment::Environment;
pub use host::{HostAction, HostCallback, HostContext, HostFunctions, HostReturn};
pub use imports::Imports;
pub use memory::{GuestMemory, MappingError, MemoryAccess, MemoryError, MemoryMap, PagedMemoryMap};
use wasm86_x86::CpuState;

/// Guest configuration ready to attach to an execution runtime.
/// Initial writes use backing offsets, allowing ROM and remapped RAM to be loaded
/// before the backend allocates its memories.
pub struct Machine<E> {
    context: CpuContext,
    imports: Imports<E>,
}

impl<E: Environment> Machine<E> {
    pub fn new(
        context: CpuContext,
        map: impl Into<MemoryMap>,
        environment: E,
    ) -> Result<Self, ConfigurationError> {
        let map = map.into();
        if !map.supports(context.target().profile) {
            return Err(ConfigurationError::IncompatibleMemory);
        }
        Ok(Self {
            context,
            imports: Imports::new(context.target(), map, environment),
        })
    }

    /// Functions supplied by this environment, installed when a runtime attaches it.
    pub fn host_functions_mut(&mut self) -> &mut HostFunctions<E> {
        &mut self.imports.pending_functions
    }

    pub fn context(&self) -> CpuContext {
        self.context
    }

    pub fn set_cpu(&mut self, cpu: CpuState) -> Result<(), ConfigurationError> {
        self.context = CpuContext::new(self.context.target(), cpu)?;
        Ok(())
    }

    pub fn initialize_backing(&mut self, offset: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        self.imports.initialize_backing(offset, bytes)
    }

    /// Consumes setup into its CPU context and environment bindings. A runtime
    /// allocates backing, then calls [`Imports::initialize_memory`].
    pub fn into_parts(self) -> (CpuContext, Imports<E>) {
        (self.context, self.imports)
    }
}

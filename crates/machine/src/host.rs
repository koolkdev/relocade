use crate::{
    ConfigurationError, CpuContext, Environment, GuestMemory, Imports, MemoryAccess, MemoryError,
};
pub use wasm86_x86::HostReturn;
use wasm86_x86::{CpuState, HostFunction};

/// What follows a completed synchronous host callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostAction {
    /// Reload state and continue guest execution without leaving generated Wasm.
    Continue,
    /// Return `ExecutionExit::HostYield` with the callback's effects published.
    /// The next run resumes its guest continuation without repeating the callback.
    Yield,
}

/// Scoped access during a direct Wasm host call. No engine handles or live memory
/// views escape this scope; routing changes are visible before Wasm resumes.
/// Callbacks cannot reenter the same machine. Asynchronous work can be recorded
/// in device state and completed between runs after returning [`HostAction::Yield`].
pub struct HostContext<'a, D> {
    cpu: &'a mut CpuState,
    imports: &'a mut Imports<D>,
    memory: &'a mut dyn GuestMemory,
}

impl<D: Environment> HostContext<'_, D> {
    pub fn cpu(&self) -> CpuState {
        *self.cpu
    }

    /// Replaces architectural state, retaining the runtime's retirement counter.
    /// The active target’s segment assumptions must remain valid.
    pub fn set_cpu(&mut self, mut cpu: CpuState) -> Result<(), ConfigurationError> {
        CpuContext::new(self.imports.execution_target(), cpu)?;
        cpu.instruction_count = self.cpu.instruction_count;
        *self.cpu = cpu;
        Ok(())
    }

    pub fn with_environment<R>(&mut self, access: impl FnOnce(&mut D) -> R) -> R {
        access(self.imports.environment_mut())
    }

    pub fn read_memory(
        &mut self,
        address: u32,
        bytes: &mut [u8],
    ) -> std::result::Result<(), MemoryError> {
        self.imports.read(self.memory, address, bytes)
    }

    pub fn write_memory(
        &mut self,
        address: u32,
        bytes: &[u8],
    ) -> std::result::Result<(), MemoryError> {
        self.imports.write(self.memory, address, bytes)
    }

    pub fn with_memory<R>(&mut self, access: impl FnOnce(&mut MemoryAccess<'_>) -> R) -> R {
        self.imports.access(self.memory, |_, memory| access(memory))
    }
}

/// Services compiled together into one Wasm module. Each binding becomes one
/// fixed host import, called directly by its generated entry function.
pub struct HostFunctions<D> {
    definitions: Vec<HostFunction>,
    callbacks: Vec<HostCallback<D>>,
}

impl<D> Default for HostFunctions<D> {
    fn default() -> Self {
        Self {
            definitions: Vec::new(),
            callbacks: Vec::new(),
        }
    }
}

impl<D> HostFunctions<D> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn definitions(&self) -> &[HostFunction] {
        &self.definitions
    }

    /// Transfers callbacks to the backend after their functions have been compiled.
    pub fn into_callbacks(self) -> Vec<HostCallback<D>> {
        self.callbacks
    }

    /// Adds a service at a logical linear address with its lowered return ABI.
    pub fn insert(
        &mut self,
        address: u32,
        return_: HostReturn,
        callback: impl FnMut(&mut HostContext<'_, D>) -> HostAction + 'static,
    ) {
        self.definitions.push(HostFunction { address, return_ });
        self.callbacks.push(HostCallback(Box::new(callback)));
    }
}

/// An owned callback for binding one generated module import. The import has
/// no runtime ID parameter; adapters bind a fixed callback slot when installing it.
pub struct HostCallback<D>(Box<HostHandler<D>>);

type HostHandler<D> = dyn FnMut(&mut HostContext<'_, D>) -> HostAction;

impl<D: Environment> Imports<D> {
    /// Retains a module's callback for the lifetime of its callable functions.
    pub fn bind_host_call(&mut self, callback: HostCallback<D>) -> usize {
        let index = self.callbacks.len();
        self.callbacks.push(Some(callback));
        index
    }

    /// Invokes the fixed binding captured by an adapter's import closure. Indexing
    /// is direct; no address lookup or host-ID map runs on the service path.
    pub fn host_call(
        &mut self,
        cpu: &mut CpuState,
        memory: &mut dyn GuestMemory,
        index: usize,
    ) -> u32 {
        let mut callback = self.callbacks[index]
            .take()
            .expect("callback cannot reenter its machine");
        let action = (callback.0)(&mut HostContext {
            cpu,
            imports: self,
            memory,
        });
        self.callbacks[index] = Some(callback);
        match action {
            HostAction::Continue => 0,
            HostAction::Yield => 1,
        }
    }
}

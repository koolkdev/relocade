use crate::{
    Cpu, CpuBindings, CpuConfig, MachineConfig, Memory, Result, RuntimeBackend, RuntimeState,
};
use relocade_wasm_runtime::{WasmBackend, WasmSharedMemory};
use std::sync::Arc;

/// Shared resources and CPU lifecycle for one emulated machine.
/// Clones refer to the same machine. A CPU retains that ownership while its
/// backend and bound instances stay on the thread that executes it.
#[derive(Clone)]
pub struct Machine<S: WasmSharedMemory> {
    pub(crate) memory: Memory<S>,
}

impl<S: WasmSharedMemory> Machine<S> {
    /// Allocates the machine's shared backing with its address policy.
    pub fn new<B: WasmBackend<SharedMemory = S>>(
        backend: &mut B,
        config: MachineConfig,
    ) -> Result<Self> {
        Ok(Self {
            memory: Memory::new(backend, config)?,
        })
    }

    /// Creates a separate machine with its own address policy over existing backing.
    pub fn with_backing(config: MachineConfig, backing: S) -> Result<Self> {
        Ok(Self {
            memory: Memory::with_backing(Arc::new(config), backing)?,
        })
    }

    pub fn configuration(&self) -> &MachineConfig {
        self.memory.configuration()
    }
    /// Shares the machine's guest memory without allowing its association to be
    /// replaced. The returned handle can edit live memory and outlive this handle.
    pub fn memory(&self) -> Memory<S> {
        self.memory.clone()
    }

    /// Creates an independently executable CPU in this machine.
    /// Guest bytes are preserved.
    pub async fn create_cpu<
        T: 'static,
        B: RuntimeBackend<State = RuntimeState<T>, SharedMemory = S>,
    >(
        &self,
        mut backend: B,
        config: CpuConfig,
    ) -> Result<Cpu<B>> {
        let bindings = self.bind_cpu(&mut backend, config)?;
        Cpu::new(self.clone(), backend, bindings).await
    }

    /// Allocates a private CPU context for custom Wasm integration. Normal
    /// execution uses [`Self::create_cpu`], which also prepares dispatch.
    pub fn bind_cpu<B: WasmBackend<SharedMemory = S>>(
        &self,
        backend: &mut B,
        config: CpuConfig,
    ) -> Result<CpuBindings<B::Memory>> {
        CpuBindings::new(backend, config, self)
    }
}

use crate::{
    runtime::Runtime, Cpu, CpuBindings, CpuConfig, MachineConfig, Memory, Result, RuntimeBackend,
    RuntimeState,
};
use relocade_wasm_runtime::{WasmBackend, WasmSharedMemory};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Shared resources and CPU lifecycle for one emulated machine.
/// Clones refer to the same machine. A CPU retains that ownership while its
/// backend and bound instances stay on the thread that executes it.
#[derive(Clone)]
pub struct Machine<S: WasmSharedMemory> {
    pub(crate) memory: Memory<S>,
    pub(crate) shared: Arc<Shared>,
}

pub(crate) struct Shared {
    pub runtime: Runtime,
    pub cpus: AtomicUsize,
    pub next_cpu: AtomicUsize,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            runtime: Runtime::default(),
            cpus: AtomicUsize::new(0),
            next_cpu: AtomicUsize::new(0),
        }
    }
}

impl<S: WasmSharedMemory> Machine<S> {
    /// Allocates the machine's shared backing, mappings and descriptor storage.
    pub fn new<B: WasmBackend<SharedMemory = S>>(
        backend: &mut B,
        config: MachineConfig,
    ) -> Result<Self> {
        Ok(Self {
            memory: Memory::new(backend, config)?,
            shared: Arc::default(),
        })
    }

    /// Creates a separate machine over existing backing, with its own mappings,
    /// descriptor storage and code identity.
    pub fn with_backing<B: WasmBackend<SharedMemory = S>>(
        backend: &mut B,
        config: MachineConfig,
        backing: S,
    ) -> Result<Self> {
        Ok(Self {
            memory: Memory::with_backing(backend, config, backing)?,
            shared: Arc::default(),
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

    /// Number of live CPU contexts, including bindings retained by host callbacks.
    pub fn cpu_count(&self) -> usize {
        self.shared.cpus.load(Ordering::Relaxed)
    }

    /// Creates an independently executable CPU in this machine.
    /// Guest bytes and shared tables are preserved.
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

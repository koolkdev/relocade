mod view;
pub use view::{MemoryError, MemoryView};

use crate::{ConfigurationError, MachineConfig, Result};
use relocade_wasm_runtime::{Backend, SharedMemory, WasmBackend, WasmSharedMemory};
use std::sync::Arc;

/// Owned guest memory and its address policy, independent of CPU execution.
/// Clones share backing bytes. Reads can overlap guest writes and do not provide
/// an atomic snapshot of a range.
#[derive(Clone)]
pub struct Memory<S: WasmSharedMemory = SharedMemory> {
    pub(crate) config: Arc<MachineConfig>,
    backing: S,
}

impl Memory {
    /// Allocates zeroed guest memory without creating a CPU or executable dispatch.
    pub fn new(config: MachineConfig) -> Result<Self> {
        let mut backend = Backend::new()?;
        Self::allocate(&mut backend, config)
    }

    pub(crate) fn allocate(
        backend: &mut impl WasmBackend<SharedMemory = SharedMemory>,
        config: MachineConfig,
    ) -> Result<Self> {
        let minimum = config.memory_size().div_ceil(65536).max(1) as u32;
        let memory = backend.create_memory(relocade_compiler::abi::Memory {
            minimum,
            ..relocade_x86::abi::GUEST
        })?;
        let backing = backend
            .shared_memory(&memory)
            .expect("guest backing is shared");
        Self::with_backing(config, backing)
    }
}

impl<S: WasmSharedMemory> Memory<S> {
    /// Associates shared backing with its guest address policy and capacity.
    pub fn with_backing(config: MachineConfig, backing: S) -> Result<Self> {
        if backing.byte_len() < config.memory_size() {
            return Err(ConfigurationError::InsufficientMemory.into());
        }
        Ok(Self {
            config: Arc::new(config),
            backing,
        })
    }

    pub fn configuration(&self) -> &MachineConfig {
        &self.config
    }
    pub fn backing(&self) -> &S {
        &self.backing
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    pub fn view(&self) -> MemoryView<'_> {
        MemoryView::new(&self.config, self.backing.view())
    }
}

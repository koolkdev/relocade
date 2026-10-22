mod view;
pub use view::{MemoryError, MemoryView};
pub(crate) mod resources;
pub(crate) mod version;

use crate::Result;
use crate::{ConfigurationError, Descriptors, MachineConfig, MemoryMap};
use relocade_compiler::abi::Memory as WasmMemory;
use relocade_wasm_runtime::{MemoryBytes, WasmBackend, WasmSharedMemory};
use relocade_x86::abi;
use resources::MemoryResources;
use std::sync::Arc;

/// A mapping version captured with source bytes, before compiling guest code.
/// It identifies shared guest memory and its live table generation. Guest-byte
/// changes still require the embedding's existing code-invalidation policy.
#[derive(Clone, Debug)]
pub struct MemoryVersion {
    identity: Arc<()>,
    generation: u64,
}

impl MemoryVersion {
    pub(crate) fn matches(&self, identity: &Arc<()>, generation: u64) -> bool {
        Arc::ptr_eq(&self.identity, identity) && self.generation == generation
    }
}

/// Owned guest memory and live address policy, independent of CPU execution.
/// Clones share mappings and bytes. Access does not lock shared storage or provide
/// a consistent snapshot; the embedding coordinates mapping changes with its users.
#[derive(Clone)]
pub struct Memory<S: WasmSharedMemory> {
    pub(crate) config: Arc<MachineConfig>,
    pub(crate) resources: MemoryResources<S>,
    pub(crate) identity: Arc<()>,
}

impl<S: WasmSharedMemory> Memory<S> {
    pub(crate) fn new<B: WasmBackend<SharedMemory = S>>(
        backend: &mut B,
        config: MachineConfig,
    ) -> Result<Self> {
        let memory = backend.create_memory(WasmMemory {
            minimum: config.memory_size().div_ceil(65536).max(1) as u32,
            ..abi::GUEST
        })?;
        let backing = backend.shared_memory(&memory).expect("shared guest memory");
        Self::with_backing(backend, config, backing)
    }

    /// Creates guest memory over existing backing, with its own mappings.
    pub(crate) fn with_backing<B: WasmBackend<SharedMemory = S>>(
        backend: &mut B,
        config: MachineConfig,
        backing: S,
    ) -> Result<Self> {
        if backing.byte_len() < config.memory_size() {
            return Err(ConfigurationError::InsufficientMemory.into());
        }
        let mapping = match config.memory_map() {
            MemoryMap::Physical(_) => None,
            MemoryMap::Paged(map) => {
                let mapping = backend.create_memory(abi::PAGE_TABLE)?;
                backend.write_memory(&mapping, 0, map.to_bytes());
                Some(backend.shared_memory(&mapping).expect("shared page table"))
            }
        };
        let revision = backend.create_memory(WasmMemory {
            minimum: 1,
            maximum: Some(1),
            ..abi::GUEST
        })?;
        let revision = backend
            .shared_memory(&revision)
            .expect("shared table version");
        let descriptors = backend.create_memory(WasmMemory {
            shared: true,
            maximum: Some(2),
            ..WasmMemory::new("relocade", "descriptors", 2)
        })?;
        if let Some(image) = config.descriptor_memory() {
            backend.write_memory(&descriptors, 0, image.to_bytes());
        }
        let descriptors = backend
            .shared_memory(&descriptors)
            .expect("shared descriptor memory");
        Self::from_resources(
            Arc::new(config),
            MemoryResources {
                backing,
                mapping,
                descriptors,
                revision,
            },
            Arc::new(()),
        )
    }

    /// Reconstructs transported allocations without reseeding tables or bytes.
    /// Preserve the machine's configuration and code identity across worker handoff.
    pub(crate) fn from_resources(
        config: Arc<MachineConfig>,
        resources: MemoryResources<S>,
        identity: Arc<()>,
    ) -> Result<Self> {
        resources.validate(&config)?;
        Ok(Self {
            config,
            resources,
            identity,
        })
    }

    /// Keeps the associated allocations together for transport through a worker.
    /// Send the original configuration too; CPU state is supplied independently.
    #[cfg(feature = "worker")]
    pub(crate) fn shared_resources(&self) -> MemoryResources<S> {
        self.resources.clone()
    }

    pub fn configuration(&self) -> &MachineConfig {
        &self.config
    }
    pub fn backing(&self) -> &S {
        &self.resources.backing
    }

    /// Borrows a read/write view of this handle without borrowing a CPU.
    pub fn view(&mut self) -> MemoryView<'_> {
        self.resources.view(&self.config)
    }

    /// Borrows shared descriptor bytes by offset, without borrowing a CPU.
    pub fn descriptors(&mut self) -> Descriptors<'_, ()> {
        Descriptors::new(MemoryBytes::shared(&self.resources.descriptors), ())
    }

    /// Copies mapped source bytes and records the current mapping version.
    /// Concurrent writes can affect the copy. Keep the version through compilation
    /// and pass it to `crate::Cpu::install_module` to detect intervening remaps.
    pub fn snapshot(
        &mut self,
        address: u32,
        bytes: &mut [u8],
    ) -> std::result::Result<MemoryVersion, MemoryError> {
        let version = MemoryVersion {
            identity: Arc::clone(&self.identity),
            generation: version::generation(&MemoryBytes::shared(&self.resources.revision)),
        };
        self.view().read(address, bytes)?;
        Ok(version)
    }
}

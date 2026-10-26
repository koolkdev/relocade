mod view;
pub use view::{MemoryError, MemoryView};

use crate::{ConfigurationError, MachineConfig, Result};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use std::sync::Arc;

/// Guest memory and its immutable address policy, independent of any CPU.
/// Clones retain direct access to the same backing allocation. Worker transport
/// shares the backing and copies the address policy to the receiving worker.
#[derive(Clone)]
#[cfg_attr(target_arch = "wasm32", derive(serde::Serialize))]
pub struct Memory {
    config: Arc<MachineConfig>,
    backing: SharedMemory,
}

impl Memory {
    /// Allocates zeroed guest memory independently of any CPU.
    pub fn new(config: MachineConfig) -> Result<Self> {
        let minimum = config.memory_size().div_ceil(65536).max(1) as u32;
        let maximum = relocade_x86::abi::GUEST
            .maximum
            .expect("guest memory has a maximum");
        Self::with_backing(config, SharedMemory::new(minimum, maximum)?)
    }

    /// Validates a shared allocation against the configured guest capacity.
    pub fn with_backing(config: MachineConfig, backing: SharedMemory) -> Result<Self> {
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

    pub fn backing(&self) -> &SharedMemory {
        &self.backing
    }

    pub fn view(&self) -> MemoryView<'_> {
        MemoryView::new(&self.config, self.backing.view())
    }
}

#[cfg(target_arch = "wasm32")]
impl<'de> serde::Deserialize<'de> for Memory {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Contents {
            config: MachineConfig,
            backing: SharedMemory,
        }
        let Contents { config, backing } = Contents::deserialize(deserializer)?;
        Self::with_backing(config, backing).map_err(serde::de::Error::custom)
    }
}

#[cfg(all(target_arch = "wasm32", feature = "worker"))]
impl relocade_wasm_runtime::worker::Post for Memory {}

use super::ConfigurationError;
use relocade_x86::{CompilerConfig, EntryAssumptions, MemoryBackend, PhysicalMemoryMap};
use std::sync::Arc;

/// Initial backing capacity and mappings, independent of a CPU's state.
#[derive(Clone)]
pub struct MachineConfig {
    memory_size: u64,
    map: Arc<PhysicalMemoryMap>,
}

impl MachineConfig {
    /// Validates backing capacity without allocating memory.
    pub fn new(memory_size: u64, map: PhysicalMemoryMap) -> Result<Self, ConfigurationError> {
        if memory_size > 1 << 32 {
            return Err(ConfigurationError::InvalidMemorySize);
        }
        if !map.fits_backing(memory_size) {
            return Err(ConfigurationError::InsufficientMemory);
        }
        Ok(Self {
            memory_size,
            map: Arc::new(map),
        })
    }

    /// Checks the CPU entry contract and the mapping ABI supplied by this machine.
    pub(crate) fn validate_compiler_config(
        &self,
        config: CompilerConfig,
    ) -> Result<(), ConfigurationError> {
        if config.entry != EntryAssumptions::REAL_MODE {
            return Err(ConfigurationError::UnsupportedMode);
        }
        if config.memory_backend != MemoryBackend::PhysicalBus {
            return Err(ConfigurationError::UnsupportedMemoryBackend);
        }
        Ok(())
    }

    pub fn memory_size(&self) -> u64 {
        self.memory_size
    }

    pub fn memory_map(&self) -> &PhysicalMemoryMap {
        &self.map
    }
}

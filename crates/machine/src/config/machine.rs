use super::ConfigurationError;
use relocade_x86::PhysicalMemoryMap;
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

    pub(crate) fn validate_for(
        &self,
        mode: relocade_x86::CpuMode,
    ) -> Result<(), ConfigurationError> {
        if mode != relocade_x86::CpuMode::Real {
            return Err(ConfigurationError::UnsupportedMode);
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

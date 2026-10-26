use super::ConfigurationError;
use relocade_x86::PhysicalMemoryMap;
use std::sync::Arc;

/// Initial backing capacity and mappings, independent of a CPU's state.
#[derive(Clone)]
#[cfg_attr(target_arch = "wasm32", derive(serde::Serialize))]
pub struct MachineConfig {
    memory_size: u64,
    map: Arc<PhysicalMemoryMap>,
}

#[cfg(target_arch = "wasm32")]
impl<'de> serde::Deserialize<'de> for MachineConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Contents {
            memory_size: u64,
            map: PhysicalMemoryMap,
        }
        let Contents { memory_size, map } = Contents::deserialize(deserializer)?;
        Self::new(memory_size, map).map_err(serde::de::Error::custom)
    }
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
        profile: relocade_x86::ExecutionProfile,
    ) -> Result<(), ConfigurationError> {
        if profile != relocade_x86::ExecutionProfile::Real16 {
            return Err(ConfigurationError::UnsupportedProfile);
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

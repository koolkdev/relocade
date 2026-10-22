use super::ConfigurationError;
use crate::{DescriptorError, DescriptorMemory, MemoryMap};
use relocade_x86::ExecutionProfile;
use std::sync::Arc;

/// Initial backing capacity and mappings, independent of a CPU's state.
#[derive(Clone)]
pub struct MachineConfig {
    memory_size: u64,
    map: Arc<MemoryMap>,
    descriptors: Option<Arc<DescriptorMemory>>,
}

impl MachineConfig {
    /// Validates backing capacity without allocating memory.
    pub fn new(memory_size: u64, map: impl Into<MemoryMap>) -> Result<Self, ConfigurationError> {
        let map = map.into();
        if memory_size > 1 << 32 {
            return Err(ConfigurationError::InvalidMemorySize);
        }
        if !map.fits_backing(memory_size) {
            return Err(ConfigurationError::InsufficientMemory);
        }
        Ok(Self {
            memory_size,
            map: Arc::new(map),
            descriptors: None,
        })
    }

    pub(crate) fn validate_for(&self, profile: ExecutionProfile) -> Result<(), ConfigurationError> {
        if !matches!(
            (profile, self.memory_map()),
            (ExecutionProfile::Real16, MemoryMap::Physical(_))
                | (ExecutionProfile::Protected(_), MemoryMap::Paged(_))
        ) {
            return Err(ConfigurationError::MemoryMapMismatch);
        }
        Ok(())
    }

    pub fn memory_size(&self) -> u64 {
        self.memory_size
    }

    pub fn memory_map(&self) -> &MemoryMap {
        &self.map
    }

    /// Sets the initial descriptor image without changing any CPU's registers.
    pub fn with_descriptor_memory(
        mut self,
        image: DescriptorMemory,
    ) -> Result<Self, DescriptorError> {
        if !matches!(self.memory_map(), MemoryMap::Paged(_)) {
            return Err(DescriptorError::RealMode);
        }
        self.descriptors = Some(Arc::new(image));
        Ok(self)
    }

    /// No image means the descriptor allocation starts zeroed.
    pub fn descriptor_memory(&self) -> Option<&DescriptorMemory> {
        self.descriptors.as_deref()
    }
}

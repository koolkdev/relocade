use super::ConfigurationError;
use relocade_x86::{
    page_table, ExecutionProfile, ExecutionTarget, MemoryBackend, PhysicalMemoryMap, SegmentProfile,
};
use std::sync::Arc;

/// Initial backing capacity and mappings, independent of a CPU's state.
#[derive(Clone)]
pub struct MachineConfig {
    memory_size: u64,
    physical_map: Option<Arc<PhysicalMemoryMap>>,
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
            physical_map: Some(Arc::new(map)),
        })
    }

    /// Maps whole 4-KiB pages of read/write RAM at identical linear and backing
    /// addresses for a flat protected-mode guest. Higher addresses are unmapped.
    pub fn flat(memory_size: u64) -> Result<Self, ConfigurationError> {
        if memory_size > 1 << 32 {
            return Err(ConfigurationError::InvalidMemorySize);
        }
        if !memory_size.is_multiple_of(u64::from(page_table::PAGE_BYTES)) {
            return Err(ConfigurationError::UnalignedMemorySize);
        }
        Ok(Self {
            memory_size,
            physical_map: None,
        })
    }

    /// Checks the CPU entry contract and the mapping ABI supplied by this machine.
    pub(crate) fn validate_target(
        &self,
        target: ExecutionTarget,
    ) -> Result<(), ConfigurationError> {
        let profile = if self.physical_map.is_some() {
            ExecutionProfile::Real16
        } else {
            ExecutionProfile::Protected(SegmentProfile::Flat32)
        };
        if target.profile != profile {
            return Err(ConfigurationError::UnsupportedMode);
        }
        if target.memory_backend != MemoryBackend::default_for(profile) {
            return Err(ConfigurationError::UnsupportedMemoryBackend);
        }
        Ok(())
    }

    pub fn memory_size(&self) -> u64 {
        self.memory_size
    }

    /// Returns the PC physical bus map, when present.
    pub fn physical_map(&self) -> Option<&PhysicalMemoryMap> {
        self.physical_map.as_deref()
    }

    /// Returns the physical address map of a PC configuration.
    ///
    /// # Panics
    /// Panics for a flat protected-mode configuration. Use [`Self::physical_map`]
    /// when the memory configuration can have either address policy.
    pub fn memory_map(&self) -> &PhysicalMemoryMap {
        self.physical_map().expect("PC physical memory map")
    }
}

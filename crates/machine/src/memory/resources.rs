use super::version;
use crate::MemoryView;
use crate::Result;
use crate::{ConfigurationError, MachineConfig, MemoryMap};
use relocade_wasm_runtime::{MemoryBytes, WasmSharedMemory};

/// Shared guest allocations. Views borrow these handles; worker handoff clones
/// the same allocations and transports the machine identity separately.
/// With the `worker` feature, this can be sent through `worker::Message`.
#[derive(Clone)]
#[cfg_attr(feature = "worker", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct MemoryResources<S: WasmSharedMemory> {
    pub(crate) backing: S,
    pub(crate) mapping: Option<S>,
    pub(crate) revision: S,
}

impl<S: WasmSharedMemory> MemoryResources<S> {
    pub(crate) fn view<'a>(&'a self, config: &'a MachineConfig) -> MemoryView<'a> {
        MemoryView {
            config,
            backing: MemoryBytes::shared(&self.backing),
            mapping: self.mapping.as_ref().map(MemoryBytes::shared),
            revision: MemoryBytes::shared(&self.revision),
        }
    }

    pub(crate) fn validate(&self, config: &MachineConfig) -> Result<()> {
        if self.backing.byte_len() < config.memory_size()
            || self.revision.byte_len() < version::BYTE_LEN
        {
            return Err(ConfigurationError::InsufficientMemory.into());
        }
        match (config.memory_map(), &self.mapping) {
            (MemoryMap::Physical(_), None) => Ok(()),
            (MemoryMap::Paged(_), Some(mapping))
                if mapping.byte_len() >= u64::from(relocade_x86::page_table::BYTE_LEN) =>
            {
                Ok(())
            }
            _ => Err(ConfigurationError::MemoryMapMismatch.into()),
        }
    }
}

use super::{MemoryError, PagedMemoryMap};
use wasm86_x86::{ExecutionProfile, PhysicalMapping, PhysicalMemoryMap};

/// The address-space model required by the active CPU execution mode.
#[derive(Debug)]
pub enum MemoryMap {
    Physical(Box<PhysicalMemoryMap>),
    Paged(PagedMemoryMap),
}
impl From<PhysicalMemoryMap> for MemoryMap {
    fn from(map: PhysicalMemoryMap) -> Self {
        Self::Physical(Box::new(map))
    }
}
impl From<PagedMemoryMap> for MemoryMap {
    fn from(map: PagedMemoryMap) -> Self {
        Self::Paged(map)
    }
}

impl MemoryMap {
    pub fn supports(&self, profile: ExecutionProfile) -> bool {
        matches!(
            (self, profile),
            (Self::Physical(_), ExecutionProfile::Real16)
                | (Self::Paged(_), ExecutionProfile::Protected(_))
        )
    }
    pub fn required_backing_bytes(&self) -> u64 {
        match self {
            Self::Physical(map) => map.required_backing_bytes(),
            Self::Paged(map) => map.required_backing_bytes(),
        }
    }
    /// Name of the generated x86 mapping-memory import.
    pub fn import_name(&self) -> &'static str {
        match self {
            Self::Physical(_) => "physicalMap",
            Self::Paged(_) => "machine",
        }
    }
    pub fn metadata_len(&self) -> usize {
        match self {
            Self::Physical(_) => PhysicalMemoryMap::BYTE_LEN,
            Self::Paged(_) => PagedMemoryMap::BYTE_LEN,
        }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Physical(map) => map.to_bytes().to_vec(),
            Self::Paged(map) => map.to_bytes(),
        }
    }
    pub(crate) fn address_limit(&self) -> u64 {
        match self {
            Self::Physical(_) => PhysicalMemoryMap::PAGE_COUNT as u64 * 4096,
            Self::Paged(_) => 1 << 32,
        }
    }
    pub(crate) fn bus_address(&self, address: u32) -> u32 {
        match self {
            Self::Physical(map) => map.bus_address(address),
            Self::Paged(_) => address,
        }
    }
    pub(crate) fn resolve(
        &self,
        address: u32,
        write: bool,
    ) -> Result<PhysicalMapping, MemoryError> {
        match self {
            Self::Physical(map) => Ok(map.get(address)),
            Self::Paged(map) => {
                let entry = map.entry(address);
                if entry & 1 == 0 {
                    return Err(MemoryError::Unmapped { address });
                }
                if write && entry & 2 == 0 {
                    return Err(MemoryError::ReadOnly { address });
                }
                let backing_offset = (entry & !4095) | (address & 4095);
                Ok(if entry & 2 != 0 {
                    PhysicalMapping::Ram { backing_offset }
                } else {
                    PhysicalMapping::Rom { backing_offset }
                })
            }
        }
    }
}

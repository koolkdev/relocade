use wasm86_x86::{ExecutionProfile, PagedMemoryMap, PhysicalMemoryMap};

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
    pub(crate) fn fits_backing(&self, memory_size: u64) -> bool {
        match self {
            Self::Physical(map) => map.fits_backing(memory_size),
            Self::Paged(map) => map.fits_backing(memory_size),
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
}

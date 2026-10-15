mod map;
mod paged;
pub use map::MemoryMap;
pub use paged::{MappingError, PagedMemoryMap};
use std::fmt;

/// Host memory setup/access failures, independent of guest CPU exceptions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    AddressRange,
    Unmapped { address: u32 },
    ReadOnly { address: u32 },
    IncompatibleMap,
    BackingRange,
    InsufficientBacking { required: u64, available: u64 },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddressRange => f.write_str("transfer exceeds the configured address space"),
            Self::Unmapped { address } => write!(f, "unmapped guest address {address:#x}"),
            Self::ReadOnly { address } => write!(f, "guest address {address:#x} is read-only"),
            Self::IncompatibleMap => {
                f.write_str("replacement map must retain the address-space model")
            }
            Self::BackingRange => f.write_str("transfer exceeds guest backing"),
            Self::InsufficientBacking {
                required,
                available,
            } => {
                write!(
                    f,
                    "memory map requires {required} backing bytes; only {available} are allocated"
                )
            }
        }
    }
}

impl std::error::Error for MemoryError {}

/// Scoped guest backing and routing for the embedding or a device callback.
/// Backing offsets are independent of physical bus addresses. CPU backing and
/// execution are absent; backing views cannot escape into a later guest entry.
pub struct MemoryAccess<'a> {
    pub(crate) backing: &'a mut dyn GuestMemory,
    pub(crate) map: &'a mut MemoryMap,
    pub(crate) changed: &'a mut bool,
}

impl MemoryAccess<'_> {
    /// Uses u64 because a valid backing can span the entire 32-bit address space.
    pub fn backing_len(&self) -> u64 {
        self.backing.byte_len()
    }

    pub fn read_backing(&self, offset: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        self.check_backing(offset, bytes.len())?;
        self.backing.read(offset, bytes);
        Ok(())
    }

    /// Writes raw backing, including ROM initialization. Validate the complete
    /// transfer before effects; this does not apply physical routing or A20.
    pub fn write_backing(&mut self, offset: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        self.check_backing(offset, bytes.len())?;
        self.backing.write(offset, bytes);
        Ok(())
    }

    fn check_backing(&self, offset: u32, bytes: usize) -> Result<(), MemoryError> {
        check_range(offset, bytes, self.backing_len())
            .then_some(())
            .ok_or(MemoryError::BackingRange)
    }

    pub fn map(&self) -> &MemoryMap {
        self.map
    }

    pub fn set_a20_enabled(&mut self, enabled: bool) -> Result<(), MemoryError> {
        let MemoryMap::Physical(map) = self.map else {
            return Err(MemoryError::IncompatibleMap);
        };
        map.set_a20_enabled(enabled);
        *self.changed = true;
        Ok(())
    }

    /// Changes routing using already allocated backing. To grow backing, replace
    /// the map through the execution session between entries.
    pub fn set_map(&mut self, map: impl Into<MemoryMap>) -> Result<(), MemoryError> {
        let map = map.into();
        if map.import_name() != self.map.import_name() {
            return Err(MemoryError::IncompatibleMap);
        }
        let required = map.required_backing_bytes();
        let available = self.backing_len();
        if required > available {
            return Err(MemoryError::InsufficientBacking {
                required,
                available,
            });
        }
        *self.map = map;
        *self.changed = true;
        Ok(())
    }
}

pub(crate) fn check_range(address: u32, bytes: usize, limit: u64) -> bool {
    u64::from(address) <= limit && bytes as u64 <= limit - u64::from(address)
}

/// Scoped backing supplied by an execution backend. Validated access ranges must
/// fit byte_len. Mapping publication completes before the next guest transfer.
pub trait GuestMemory {
    fn byte_len(&self) -> u64;
    fn read(&self, offset: u32, bytes: &mut [u8]);
    fn write(&mut self, offset: u32, bytes: &[u8]);
    fn publish_map(&mut self, bytes: &[u8]);
}

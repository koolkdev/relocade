use crate::backend::GuestMemory;
use std::fmt;
use wasm86_x86::PhysicalMemoryMap;

/// Host memory setup/access failures, independent of guest CPU exceptions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    AddressRange,
    BackingRange,
    InsufficientBacking { required: u64, available: u64 },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddressRange => {
                f.write_str("physical transfer exceeds the real-mode address range")
            }
            Self::BackingRange => f.write_str("transfer exceeds guest backing"),
            Self::InsufficientBacking {
                required,
                available,
            } => {
                write!(f,
                "physical map requires {required} backing bytes; only {available} are allocated")
            }
        }
    }
}

impl std::error::Error for MemoryError {}

/// Scoped guest backing and routing for the embedding or a device callback.
/// Backing offsets are independent of physical bus addresses. CPU backing and
/// execution are absent; backing views cannot escape into a later guest entry.
pub struct DeviceMemory<'a> {
    pub(crate) backing: &'a mut dyn GuestMemory,
    pub(crate) map: &'a mut PhysicalMemoryMap,
    pub(crate) changed: &'a mut bool,
}

impl DeviceMemory<'_> {
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

    pub fn map(&self) -> &PhysicalMemoryMap {
        self.map
    }

    pub fn set_a20_enabled(&mut self, enabled: bool) {
        self.map.set_a20_enabled(enabled);
        *self.changed = true;
    }

    /// Changes routing using already allocated backing. To grow backing, replace
    /// the map through [`crate::Machine::set_memory_map`] between entries.
    pub fn set_map(&mut self, map: PhysicalMemoryMap) -> Result<(), MemoryError> {
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

pub(crate) fn check_physical_range(address: u32, bytes: usize) -> Result<(), MemoryError> {
    let limit = PhysicalMemoryMap::PAGE_COUNT as u64 * u64::from(PhysicalMemoryMap::PAGE_BYTES);
    check_range(address, bytes, limit)
        .then_some(())
        .ok_or(MemoryError::AddressRange)
}

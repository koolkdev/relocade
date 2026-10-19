use crate::{MachineConfig, MachineStorage};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::fmt;

/// Guest physical memory through the configured map, including A20 aliases.
/// RAM/ROM use backing storage. Holes and currently unattached MMIO return FF
/// bytes; writes to ROM, holes and unattached MMIO are ignored.
pub struct Memory<'a, S: MachineStorage + ?Sized = dyn MachineStorage + 'a> {
    config: &'a MachineConfig,
    storage: &'a mut S,
}

impl<'a, S: MachineStorage + ?Sized> Memory<'a, S> {
    /// Borrows backing storage matching the configured map and capacity.
    pub fn new(config: &'a MachineConfig, storage: &'a mut S) -> Self {
        Self { config, storage }
    }

    /// Reads guest addresses. Addresses outside the fixed map are holes.
    pub fn read(&mut self, mut address: u32, mut bytes: &mut [u8]) -> Result<(), MemoryError> {
        if !range_fits(address, bytes.len(), 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            let (part, rest) = bytes.split_at_mut(count);
            match self.config.memory_map().get(address) {
                PhysicalMapping::Ram { backing_offset }
                | PhysicalMapping::Rom { backing_offset } => {
                    self.storage.read_backing(backing_offset, part);
                }
                PhysicalMapping::Unmapped | PhysicalMapping::Mmio => part.fill(0xff),
            }
            address = address.wrapping_add(count as u32);
            bytes = rest;
        }
        Ok(())
    }

    /// Writes guest addresses with the same mapping and ROM policy as execution.
    pub fn write(&mut self, mut address: u32, mut bytes: &[u8]) -> Result<(), MemoryError> {
        if !range_fits(address, bytes.len(), 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            if let PhysicalMapping::Ram { backing_offset } = self.config.memory_map().get(address) {
                self.storage.write_backing(backing_offset, &bytes[..count]);
            }
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
        Ok(())
    }

    /// Configured backing capacity, excluding Wasm-page rounding.
    pub fn backing_size(&self) -> u64 {
        self.config.memory_size()
    }

    /// Reads storage by backing offset, bypassing guest address translation.
    pub fn read_backing(&self, offset: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        if !range_fits(offset, bytes.len(), self.backing_size()) {
            return Err(MemoryError::BackingRange);
        }
        self.storage.read_backing(offset, bytes);
        Ok(())
    }

    /// Initializes storage directly, including ROM contents.
    pub fn write_backing(&mut self, offset: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        if !range_fits(offset, bytes.len(), self.backing_size()) {
            return Err(MemoryError::BackingRange);
        }
        self.storage.write_backing(offset, bytes);
        Ok(())
    }
}

fn page_span(address: u32, remaining: usize) -> usize {
    remaining
        .min((PhysicalMemoryMap::PAGE_BYTES - address % PhysicalMemoryMap::PAGE_BYTES) as usize)
}

fn range_fits(address: u32, len: usize, limit: u64) -> bool {
    u64::from(address)
        .checked_add(len as u64)
        .is_some_and(|end| end <= limit)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    AddressRange,
    BackingRange,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AddressRange => "memory access exceeds the 32-bit guest address space",
            Self::BackingRange => "backing access exceeds memory_size",
        })
    }
}

impl std::error::Error for MemoryError {}

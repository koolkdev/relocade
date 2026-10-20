use crate::{MachineConfig, MachineStorage};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap, SegmentRegister};
use std::fmt;

/// Read access to guest physical memory, independent of CPU ownership.
/// Implementations supply the fixed map and checked backing transfers.
pub trait ReadMemory {
    fn memory_map(&self) -> &PhysicalMemoryMap;
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) -> Result<(), MemoryError>;

    /// Reads mapped RAM/ROM, including A20 aliases. Holes and unattached MMIO
    /// return FF bytes. Concurrent readers need not observe one coherent snapshot.
    fn read(&self, mut address: u32, mut bytes: &mut [u8]) -> Result<(), MemoryError> {
        if !range_fits(address, bytes.len(), 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            let (part, rest) = bytes.split_at_mut(count);
            match self.memory_map().get(address) {
                PhysicalMapping::Ram { backing_offset }
                | PhysicalMapping::Rom { backing_offset } => {
                    self.read_backing(backing_offset, part)?;
                }
                PhysicalMapping::Unmapped | PhysicalMapping::Mmio => part.fill(0xff),
            }
            address = address.wrapping_add(count as u32);
            bytes = rest;
        }
        Ok(())
    }

    fn read_u8(&self, address: u32) -> Result<u8, MemoryError> {
        let mut bytes = [0];
        self.read(address, &mut bytes)?;
        Ok(bytes[0])
    }

    fn read_u16(&self, address: u32) -> Result<u16, MemoryError> {
        let mut bytes = [0; 2];
        self.read(address, &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }
}

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
    pub fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        ReadMemory::read(self, address, bytes)
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

    /// Reads one byte through the guest memory map.
    pub fn read_u8(&self, address: u32) -> Result<u8, MemoryError> {
        ReadMemory::read_u8(self, address)
    }

    /// Reads a little-endian word through the guest memory map.
    pub fn read_u16(&self, address: u32) -> Result<u16, MemoryError> {
        ReadMemory::read_u16(self, address)
    }

    /// Writes one byte with the same guest access policy as [`Self::write`].
    pub fn write_u8(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        self.write(address, &[value])
    }

    /// Writes a little-endian word with the same guest access policy as [`Self::write`].
    pub fn write_u16(&mut self, address: u32, value: u16) -> Result<(), MemoryError> {
        self.write(address, &value.to_le_bytes())
    }

    /// Reads a contiguous span through a real-mode segment cache.
    /// Checks the entire span before copying; never wraps or truncates the offset.
    /// Physical mappings, including A20, apply after segment translation.
    pub fn read_segment(
        &mut self,
        segment: &SegmentRegister,
        offset: u32,
        bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        let address = segment_address(segment, offset, bytes.len())?;
        self.read(address, bytes)
    }

    /// Writes a contiguous span through a real-mode segment cache.
    /// As with execution in real mode, the limit applies but protected-mode access
    /// permissions do not: a CS-relative write is allowed.
    pub fn write_segment(
        &mut self,
        segment: &SegmentRegister,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), MemoryError> {
        let address = segment_address(segment, offset, bytes.len())?;
        self.write(address, bytes)
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

impl<S: MachineStorage + ?Sized> ReadMemory for Memory<'_, S> {
    fn memory_map(&self) -> &PhysicalMemoryMap {
        self.config.memory_map()
    }

    fn read_backing(&self, offset: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        self.read_backing(offset, bytes)
    }
}

fn segment_address(segment: &SegmentRegister, offset: u32, len: usize) -> Result<u32, MemoryError> {
    if !range_fits(offset, len, u64::from(segment.limit) + 1) {
        return Err(MemoryError::SegmentRange);
    }
    segment
        .base
        .checked_add(offset)
        .ok_or(MemoryError::AddressRange)
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
    SegmentRange,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AddressRange => "memory access exceeds the 32-bit guest address space",
            Self::BackingRange => "backing access exceeds memory_size",
            Self::SegmentRange => "memory access exceeds the segment limit",
        })
    }
}

impl std::error::Error for MemoryError {}

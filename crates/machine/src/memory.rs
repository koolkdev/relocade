mod paged;

use crate::{MachineConfig, MachineStorage, MemoryMap};
use relocade_x86::{
    Exception, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, StoredSegment,
};
use std::fmt;

/// Guest addresses through the machine's memory policy. Real-mode physical
/// routing includes A20, ROM and holes. Protected-user accesses use the live
/// page map and report page faults before copying any bytes.
pub struct Memory<'a, S: MachineStorage + ?Sized = dyn MachineStorage + 'a> {
    config: &'a MachineConfig,
    storage: &'a mut S,
}

impl<'a, S: MachineStorage + ?Sized> Memory<'a, S> {
    /// Borrows backing storage matching the configured map and capacity.
    pub fn new(config: &'a MachineConfig, storage: &'a mut S) -> Self {
        Self { config, storage }
    }

    /// Reads a nonwrapping guest span. Physical addresses outside the map are holes.
    pub fn read(&mut self, mut address: u32, mut bytes: &mut [u8]) -> Result<(), MemoryError> {
        if !range_fits(address, bytes.len(), 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        let MemoryMap::Physical(map) = self.config.memory_map() else {
            return self
                .read_paged(address, bytes)
                .map_err(MemoryError::PageFault);
        };
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            let (part, rest) = bytes.split_at_mut(count);
            match map.get(address) {
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
        let MemoryMap::Physical(map) = self.config.memory_map() else {
            return self
                .write_paged(address, bytes)
                .map_err(MemoryError::PageFault);
        };
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            if let PhysicalMapping::Ram { backing_offset } = map.get(address) {
                self.storage.write_backing(backing_offset, &bytes[..count]);
            }
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
        Ok(())
    }

    /// Reads a complete span through a loaded segment cache, then the active map.
    /// Protected mode enforces usability, readability and expand-down bounds.
    /// Host slices never wrap or truncate their segment offset or linear span.
    pub fn read_segment(
        &mut self,
        segment: &StoredSegment,
        offset: u32,
        bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        let address = segment_address(
            segment,
            offset,
            bytes.len(),
            self.config.execution_target().profile,
            false,
        )?;
        self.read(address, bytes)
    }

    /// Writes through a loaded segment cache and the active map. Protected mode
    /// checks write permission; real mode permits writes through any segment.
    pub fn write_segment(
        &mut self,
        segment: &StoredSegment,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), MemoryError> {
        let address = segment_address(
            segment,
            offset,
            bytes.len(),
            self.config.execution_target().profile,
            true,
        )?;
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

fn segment_address(
    segment: &StoredSegment,
    offset: u32,
    len: usize,
    profile: ExecutionProfile,
    write: bool,
) -> Result<u32, MemoryError> {
    if matches!(profile, ExecutionProfile::Protected(_)) {
        let permission = if write {
            segment.writable
        } else {
            segment.readable
        };
        if len != 0 && (segment.usable == 0 || permission == 0) {
            return Err(MemoryError::SegmentAccess);
        }
        if segment.expand_down != 0 {
            let upper = if segment.big != 0 { 1 << 32 } else { 1 << 16 };
            if len != 0 && (offset <= segment.limit || !range_fits(offset, len, upper)) {
                return Err(MemoryError::SegmentRange);
            }
        } else if !range_fits(offset, len, u64::from(segment.limit) + 1) {
            return Err(MemoryError::SegmentRange);
        }
        Ok(segment.base.wrapping_add(offset))
    } else {
        if !range_fits(offset, len, u64::from(segment.limit) + 1) {
            return Err(MemoryError::SegmentRange);
        }
        segment
            .base
            .checked_add(offset)
            .ok_or(MemoryError::AddressRange)
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
    SegmentRange,
    SegmentAccess,
    PageFault(Exception),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::PageFault(exception) = self {
            return exception.fmt(f);
        }
        f.write_str(match self {
            Self::AddressRange => "memory access exceeds the 32-bit guest address space",
            Self::BackingRange => "backing access exceeds memory_size",
            Self::PageFault(_) => unreachable!(),
            Self::SegmentAccess => "segment does not permit this access",
            Self::SegmentRange => "memory access exceeds the segment limit",
        })
    }
}

impl std::error::Error for MemoryError {}

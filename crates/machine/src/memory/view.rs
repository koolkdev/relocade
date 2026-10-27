mod read;
mod resolve;
mod write;

use crate::{IoBus, MachineConfig};
use relocade_wasm_runtime::MemoryAccess;
use relocade_x86::SegmentRegister;
use std::fmt;

/// Guest physical memory through the configured map, including A20 aliases.
/// RAM/ROM use backing storage. MMIO requires a local I/O bus. Holes and
/// unconnected device ranges return FF; writes to them and ROM are ignored.
/// The view borrows one storage handle; other shared handles remain usable.
pub struct MemoryView<'a> {
    config: &'a MachineConfig,
    storage: MemoryAccess<'a>,
    io: Option<&'a IoBus>,
}

impl<'a> MemoryView<'a> {
    /// Borrows backing storage matching the configured map and capacity.
    #[inline]
    pub fn new(config: &'a MachineConfig, storage: MemoryAccess<'a>) -> Self {
        Self {
            config,
            storage,
            io: None,
        }
    }

    /// Enables device accesses through the execution worker's bus. CPU contexts
    /// already supply it; independent RAM views need not retain any devices.
    pub fn with_io(mut self, io: &'a IoBus) -> Self {
        self.io = Some(io);
        self
    }

    /// Configured backing capacity, excluding Wasm-page rounding.
    pub fn backing_size(&self) -> u64 {
        self.config.memory_size()
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
    DeviceAccess,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AddressRange => "memory access exceeds the 32-bit guest address space",
            Self::BackingRange => "backing access exceeds memory_size",
            Self::SegmentRange => "memory access exceeds the segment limit",
            Self::DeviceAccess => "MMIO access requires the execution worker's device bus",
        })
    }
}

impl std::error::Error for MemoryError {}

#[cfg(test)]
mod tests;

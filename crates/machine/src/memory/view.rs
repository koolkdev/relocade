use relocade_wasm_runtime::MemoryBytes;
mod paged;
mod read;
mod write;

use crate::{MachineConfig, MemoryMap};
use relocade_x86::{Exception, PhysicalMemoryMap, SegmentRegister};
use std::fmt;

/// Guest addresses through the machine's memory policy. Real-mode physical
/// routing includes A20, ROM and holes. Protected-user accesses use the live
/// page map and report page faults before copying any bytes.
/// The view borrows one storage handle; other shared handles remain usable.
pub struct MemoryView<'a> {
    pub(crate) config: &'a MachineConfig,
    pub(crate) backing: MemoryBytes<'a>,
    pub(crate) mapping: Option<MemoryBytes<'a>>,
    pub(crate) revision: MemoryBytes<'a>,
    pub(crate) descriptors: MemoryBytes<'a>,
}

impl<'a> MemoryView<'a> {
    pub(crate) fn reborrow(&mut self) -> MemoryView<'_> {
        MemoryView {
            config: self.config,
            backing: self.backing.reborrow(),
            mapping: self.mapping.as_mut().map(MemoryBytes::reborrow),
            revision: self.revision.reborrow(),
            descriptors: self.descriptors.reborrow(),
        }
    }

    /// Borrows descriptor bytes independently of CPU table registers.
    pub fn descriptors(&mut self) -> crate::Descriptors<'_, ()> {
        crate::Descriptors::new(self.descriptors.reborrow(), ())
    }

    /// Configured backing capacity, excluding Wasm-page rounding.
    pub fn backing_size(&self) -> u64 {
        self.config.memory_size()
    }
}

fn segment_address(
    segment: &SegmentRegister,
    offset: u32,
    len: usize,
    map: &MemoryMap,
    write: bool,
) -> Result<u32, MemoryError> {
    match map {
        MemoryMap::Paged(_) => {
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
        }
        MemoryMap::Physical(_) => {
            if !range_fits(offset, len, u64::from(segment.limit) + 1) {
                return Err(MemoryError::SegmentRange);
            }
            segment
                .base
                .checked_add(offset)
                .ok_or(MemoryError::AddressRange)
        }
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

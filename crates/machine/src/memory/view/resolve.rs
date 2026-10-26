use super::{range_fits, MemoryError, MemoryView};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

pub(super) struct Span {
    pub(super) backing_offset: Option<u32>,
    pub(super) len: usize,
}

#[derive(Clone, Copy)]
pub(super) enum Access {
    Read,
    Write,
}

/// Walks a checked guest range without borrowing its backing storage.
pub(super) struct Spans<'a> {
    map: &'a PhysicalMemoryMap,
    address: u32,
    remaining: usize,
    access: Access,
}

impl<'a> MemoryView<'a> {
    /// Check the whole range before any transfer can take effect.
    pub(super) fn resolve(
        &self,
        address: u32,
        len: usize,
        access: Access,
    ) -> Result<Spans<'a>, MemoryError> {
        if !range_fits(address, len, 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        Ok(Spans {
            map: self.config.memory_map(),
            address,
            remaining: len,
            access,
        })
    }
}

impl Iterator for Spans<'_> {
    type Item = Span;

    fn next(&mut self) -> Option<Span> {
        if self.remaining == 0 {
            return None;
        }
        let len = self.remaining.min(
            (PhysicalMemoryMap::PAGE_BYTES - self.address % PhysicalMemoryMap::PAGE_BYTES) as usize,
        );
        let backing_offset = match (self.map.get(self.address), self.access) {
            (PhysicalMapping::Ram { backing_offset }, _)
            | (PhysicalMapping::Rom { backing_offset }, Access::Read) => Some(backing_offset),
            _ => None,
        };
        self.remaining -= len;
        // A checked range may end exactly at 2^32; no span follows that wrap.
        self.address = self.address.wrapping_add(len as u32);
        Some(Span {
            backing_offset,
            len,
        })
    }
}

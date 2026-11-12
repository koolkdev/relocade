use super::{range_fits, MemoryError};
use crate::IoBus;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};

pub(super) struct Span {
    pub(super) mapping: PhysicalMapping,
    pub(super) address: u32,
    pub(super) len: usize,
}

/// Walks a checked guest range without borrowing its backing storage.
#[derive(Clone)]
pub(super) struct Spans<'a> {
    map: &'a PhysicalMemoryMap,
    address: u32,
    remaining: usize,
    io: Option<&'a IoBus>,
}

impl<'a> Spans<'a> {
    /// Checks the guest address range before walking its mappings.
    pub(super) fn new(
        map: &'a PhysicalMemoryMap,
        address: u32,
        len: usize,
    ) -> Result<Self, MemoryError> {
        if !range_fits(address, len, 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        Ok(Self {
            map,
            address,
            remaining: len,
            io: None,
        })
    }

    pub(super) fn with_io(mut self, io: Option<&'a IoBus>) -> Self {
        self.io = io;
        self
    }

    fn mapping(&self, address: u32) -> PhysicalMapping {
        match (self.map.get(address), self.io) {
            (PhysicalMapping::Mmio { .. }, Some(io)) => PhysicalMapping::Mmio {
                handler: io.mmio_map().page_handler(self.map.bus_address(address)),
            },
            (mapping, _) => mapping,
        }
    }
}

impl Iterator for Spans<'_> {
    type Item = Span;

    fn next(&mut self) -> Option<Span> {
        if self.remaining == 0 {
            return None;
        }
        let mut len = self.remaining.min(
            (PhysicalMemoryMap::PAGE_BYTES - self.address % PhysicalMemoryMap::PAGE_BYTES) as usize,
        );
        let mapping = self.mapping(self.address);
        let address = self.map.bus_address(self.address);
        if matches!(mapping, PhysicalMapping::Mmio { .. }) {
            // Yield after each callback so the next span sees current routing.
            let remaining = self.remaining.min(8);
            len = len.min(remaining);
            // Like guest transfers, keep a width-aware device access intact
            // across adjacent MMIO pages, but never across an A20 discontinuity.
            while len < remaining {
                let next = self.address + len as u32;
                if self.mapping(next) != mapping
                    || self.map.bus_address(next) != address + len as u32
                {
                    break;
                }
                len += (remaining - len).min(
                    (PhysicalMemoryMap::PAGE_BYTES - next % PhysicalMemoryMap::PAGE_BYTES) as usize,
                );
            }
        }
        self.remaining -= len;
        // A checked range may end exactly at 2^32; no span follows that wrap.
        self.address = self.address.wrapping_add(len as u32);
        Some(Span {
            mapping,
            address,
            len,
        })
    }
}

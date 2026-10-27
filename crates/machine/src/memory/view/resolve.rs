use super::{range_fits, MemoryError, MemoryView};
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
}

impl<'a> MemoryView<'a> {
    /// Check the whole range before any transfer can take effect.
    pub(super) fn resolve(&self, address: u32, len: usize) -> Result<Spans<'a>, MemoryError> {
        if !range_fits(address, len, 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        let spans = Spans {
            map: self.config.memory_map(),
            address,
            remaining: len,
        };
        if self.io.is_none()
            && spans
                .clone()
                .any(|span| span.mapping == PhysicalMapping::Mmio)
        {
            return Err(MemoryError::DeviceAccess);
        }
        Ok(spans)
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
        let mapping = self.map.get(self.address);
        let address = self.map.bus_address(self.address);
        if mapping == PhysicalMapping::Mmio {
            // Like guest transfers, keep a width-aware device access intact
            // across adjacent MMIO pages, but never across an A20 discontinuity.
            while len < self.remaining {
                let next = self.address + len as u32;
                if self.map.get(next) != PhysicalMapping::Mmio
                    || self.map.bus_address(next) != address + len as u32
                {
                    break;
                }
                len += (self.remaining - len).min(
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

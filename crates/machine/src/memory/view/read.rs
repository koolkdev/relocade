use super::{page_span, range_fits, segment_address, MemoryError, MemoryView};
use relocade_x86::{PhysicalMapping, SegmentRegister};

macro_rules! scalar_read {
    ($read:ident, $type:ty) => {
        /// Reads a little-endian scalar through the guest memory map.
        pub fn $read(&self, address: u32) -> Result<$type, MemoryError> {
            const BYTES: usize = size_of::<$type>();
            if !range_fits(address, BYTES, 1 << 32) {
                return Err(MemoryError::AddressRange);
            }
            if page_span(address, BYTES) == BYTES {
                return Ok(match self.config.memory_map().get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => self.storage.load(backing_offset),
                    PhysicalMapping::Unmapped | PhysicalMapping::Mmio => <$type>::MAX,
                });
            }
            // Each page can map to different backing or have a different access policy.
            let mut bytes = [0; BYTES];
            self.read(address, &mut bytes)?;
            Ok(<$type>::from_le_bytes(bytes))
        }
    };
}

impl MemoryView<'_> {
    /// Reads guest addresses using the configured map and checks the full range.
    pub fn read(&self, mut address: u32, mut bytes: &mut [u8]) -> Result<(), MemoryError> {
        if !range_fits(address, bytes.len(), 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        let map = self.config.memory_map();
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            let (part, rest) = bytes.split_at_mut(count);
            match map.get(address) {
                PhysicalMapping::Ram { backing_offset }
                | PhysicalMapping::Rom { backing_offset } => {
                    self.storage.read(backing_offset, part);
                }
                PhysicalMapping::Unmapped | PhysicalMapping::Mmio => part.fill(0xff),
            }
            address = address.wrapping_add(count as u32);
            bytes = rest;
        }
        Ok(())
    }

    scalar_read!(read_u8, u8);
    scalar_read!(read_u16, u16);
    scalar_read!(read_u32, u32);
    scalar_read!(read_u64, u64);

    /// Reads a contiguous span through a real-mode segment cache.
    /// Checks the entire span before copying; never wraps or truncates the offset.
    /// Physical mappings, including A20, apply after segment translation.
    pub fn read_segment(
        &self,
        segment: &SegmentRegister,
        offset: u32,
        bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        let address = segment_address(segment, offset, bytes.len())?;
        self.read(address, bytes)
    }

    /// Reads checked backing offsets, bypassing guest address translation.
    pub fn read_backing(&self, offset: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        if !range_fits(offset, bytes.len(), self.backing_size()) {
            return Err(MemoryError::BackingRange);
        }
        self.storage.read(offset, bytes);
        Ok(())
    }
}

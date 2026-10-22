use super::{page_span, range_fits, segment_address, MemoryError, MemoryView};
use relocade_x86::{PhysicalMapping, SegmentRegister};

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
                    self.backing.read(backing_offset, part);
                }
                PhysicalMapping::Unmapped | PhysicalMapping::Mmio => part.fill(0xff),
            }
            address = address.wrapping_add(count as u32);
            bytes = rest;
        }
        Ok(())
    }

    /// Reads one byte through the guest memory map.
    pub fn read_u8(&self, address: u32) -> Result<u8, MemoryError> {
        let mut bytes = [0];
        self.read(address, &mut bytes)?;
        Ok(bytes[0])
    }

    /// Reads a little-endian word through the guest memory map.
    pub fn read_u16(&self, address: u32) -> Result<u16, MemoryError> {
        let mut bytes = [0; 2];
        self.read(address, &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

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
        self.backing.read(offset, bytes);
        Ok(())
    }
}

use super::{page_span, range_fits, segment_address, MemoryError, MemoryView};
use crate::MemoryMap;
use relocade_x86::{PhysicalMapping, SegmentRegister};

impl MemoryView<'_> {
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
                self.backing.write(backing_offset, &bytes[..count]);
            }
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
        Ok(())
    }

    /// Writes one byte with the same guest access policy as [`Self::write`].
    pub fn write_u8(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        self.write(address, &[value])
    }

    /// Writes a little-endian word with the same guest access policy as [`Self::write`].
    pub fn write_u16(&mut self, address: u32, value: u16) -> Result<(), MemoryError> {
        self.write(address, &value.to_le_bytes())
    }

    /// Writes through a loaded segment cache and the active map. Protected mode
    /// checks write permission; real mode permits writes through any segment.
    pub fn write_segment(
        &mut self,
        segment: &SegmentRegister,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), MemoryError> {
        let address =
            segment_address(segment, offset, bytes.len(), self.config.memory_map(), true)?;
        self.write(address, bytes)
    }

    /// Initializes storage directly, including ROM contents.
    pub fn write_backing(&mut self, offset: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        if !range_fits(offset, bytes.len(), self.backing_size()) {
            return Err(MemoryError::BackingRange);
        }
        self.backing.write(offset, bytes);
        Ok(())
    }
}

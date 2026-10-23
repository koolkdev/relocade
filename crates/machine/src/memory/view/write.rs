use super::{page_span, range_fits, segment_address, MemoryError, MemoryView};
use relocade_x86::{PhysicalMapping, SegmentRegister};

macro_rules! scalar_write {
    ($write:ident, $type:ty) => {
        /// Writes a little-endian scalar with the same guest policy as [`Self::write`].
        pub fn $write(&mut self, address: u32, value: $type) -> Result<(), MemoryError> {
            const BYTES: usize = size_of::<$type>();
            if !range_fits(address, BYTES, 1 << 32) {
                return Err(MemoryError::AddressRange);
            }
            if page_span(address, BYTES) == BYTES {
                if let PhysicalMapping::Ram { backing_offset } =
                    self.config.memory_map().get(address)
                {
                    self.storage.store(backing_offset, value);
                }
                return Ok(());
            }
            self.write(address, &value.to_le_bytes())
        }
    };
}

impl MemoryView<'_> {
    /// Writes guest addresses with the same mapping and ROM policy as execution.
    pub fn write(&mut self, mut address: u32, mut bytes: &[u8]) -> Result<(), MemoryError> {
        if !range_fits(address, bytes.len(), 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            if let PhysicalMapping::Ram { backing_offset } = self.config.memory_map().get(address) {
                self.storage.write(backing_offset, &bytes[..count]);
            }
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
        Ok(())
    }

    scalar_write!(write_u8, u8);
    scalar_write!(write_u16, u16);
    scalar_write!(write_u32, u32);
    scalar_write!(write_u64, u64);

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

    /// Initializes storage directly, including ROM contents.
    pub fn write_backing(&mut self, offset: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        if !range_fits(offset, bytes.len(), self.backing_size()) {
            return Err(MemoryError::BackingRange);
        }
        self.storage.write(offset, bytes);
        Ok(())
    }
}

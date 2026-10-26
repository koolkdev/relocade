use super::{range_fits, segment_address, MemoryError, MemoryView};
use relocade_wasm_runtime::MemoryValue;
use relocade_x86::{PhysicalMapping, SegmentRegister};

impl<'a> MemoryView<'a> {
    /// Writes guest addresses with the same mapping and ROM policy as execution.
    pub fn write(&mut self, address: u32, mut bytes: &[u8]) -> Result<(), MemoryError> {
        for span in self.resolve(address, bytes.len())? {
            let (part, rest) = bytes.split_at(span.len);
            match span.mapping {
                PhysicalMapping::Ram { backing_offset } => self.storage.write(backing_offset, part),
                PhysicalMapping::Mmio => {
                    for (index, chunk) in part.chunks(8).enumerate() {
                        let mut value = [0; 8];
                        value[..chunk.len()].copy_from_slice(chunk);
                        self.io.write_mmio(
                            span.address + (index * 8) as u32,
                            chunk.len() as u8,
                            u64::from_le_bytes(value),
                        );
                    }
                }
                PhysicalMapping::Rom { .. } | PhysicalMapping::Unmapped => {}
            }
            bytes = rest;
        }
        Ok(())
    }

    /// Writes a little-endian 8-bit value with the policy of [`Self::write`].
    pub fn write_u8(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        self.store(address, value)
    }

    /// Writes a little-endian 16-bit value with the policy of [`Self::write`].
    pub fn write_u16(&mut self, address: u32, value: u16) -> Result<(), MemoryError> {
        self.store(address, value)
    }

    /// Writes a little-endian 32-bit value with the policy of [`Self::write`].
    pub fn write_u32(&mut self, address: u32, value: u32) -> Result<(), MemoryError> {
        self.store(address, value)
    }

    /// Writes a little-endian 64-bit value with the policy of [`Self::write`].
    pub fn write_u64(&mut self, address: u32, value: u64) -> Result<(), MemoryError> {
        self.store(address, value)
    }

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

    fn store<V: MemoryValue>(&mut self, address: u32, value: V) -> Result<(), MemoryError> {
        self.write(address, value.to_le_bytes().as_ref())
    }
}

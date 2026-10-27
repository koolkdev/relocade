use super::{range_fits, segment_address, MemoryError, MemoryView};
use relocade_wasm_runtime::MemoryValue;
use relocade_x86::{PhysicalMapping, SegmentRegister};

impl<'a> MemoryView<'a> {
    /// Reads guest addresses using the configured map and checks the full range.
    pub fn read(&self, address: u32, mut bytes: &mut [u8]) -> Result<(), MemoryError> {
        for span in self.resolve(address, bytes.len())? {
            let (part, rest) = bytes.split_at_mut(span.len);
            match span.mapping {
                PhysicalMapping::Ram { backing_offset }
                | PhysicalMapping::Rom { backing_offset } => {
                    self.storage.read(backing_offset, part)
                }
                PhysicalMapping::Mmio => {
                    for (index, chunk) in part.chunks_mut(8).enumerate() {
                        let value = self
                            .io
                            .expect("resolved MMIO bus")
                            .read_mmio(span.address + (index * 8) as u32, chunk.len() as u8);
                        chunk.copy_from_slice(&value.to_le_bytes()[..chunk.len()]);
                    }
                }
                PhysicalMapping::Unmapped => part.fill(0xff),
            }
            bytes = rest;
        }
        Ok(())
    }

    /// Reads a little-endian 8-bit value through the guest memory map.
    pub fn read_u8(&self, address: u32) -> Result<u8, MemoryError> {
        self.load(address)
    }

    /// Reads a little-endian 16-bit value through the guest memory map.
    pub fn read_u16(&self, address: u32) -> Result<u16, MemoryError> {
        self.load(address)
    }

    /// Reads a little-endian 32-bit value through the guest memory map.
    pub fn read_u32(&self, address: u32) -> Result<u32, MemoryError> {
        self.load(address)
    }

    /// Reads a little-endian 64-bit value through the guest memory map.
    pub fn read_u64(&self, address: u32) -> Result<u64, MemoryError> {
        self.load(address)
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
        self.storage.read(offset, bytes);
        Ok(())
    }

    fn load<V: MemoryValue>(&self, address: u32) -> Result<V, MemoryError> {
        let mut bytes = V::Bytes::default();
        self.read(address, bytes.as_mut())?;
        Ok(V::from_le_bytes(bytes))
    }
}

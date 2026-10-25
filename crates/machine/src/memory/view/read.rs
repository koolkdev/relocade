use super::{page_span, range_fits, segment_address, MemoryError, MemoryView};
use relocade_wasm_runtime::{Load, MemoryAccess, MemoryValue};
use relocade_x86::{PhysicalMapping, SegmentRegister};

impl<'a> MemoryView<'a> {
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

    /// Reads a little-endian 8-bit value through the guest memory map.
    #[inline]
    pub fn read_u8(&self, address: u32) -> Result<u8, MemoryError> {
        self.load(address)
    }

    /// Reads a little-endian 16-bit value through the guest memory map.
    #[inline]
    pub fn read_u16(&self, address: u32) -> Result<u16, MemoryError> {
        self.load(address)
    }

    /// Reads a little-endian 32-bit value through the guest memory map.
    #[inline]
    pub fn read_u32(&self, address: u32) -> Result<u32, MemoryError> {
        self.load(address)
    }

    /// Reads a little-endian 64-bit value through the guest memory map.
    #[inline]
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

    #[inline]
    fn load<V: MemoryValue>(&self, address: u32) -> Result<V, MemoryError>
    where
        MemoryAccess<'a>: Load<V>,
    {
        let mut bytes = V::Bytes::default();
        let len = bytes.as_ref().len();
        if !range_fits(address, len, 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        if page_span(address, len) == len {
            match self.config.memory_map().get(address) {
                PhysicalMapping::Ram { backing_offset }
                | PhysicalMapping::Rom { backing_offset } => {
                    return Ok(self.storage.load(backing_offset));
                }
                PhysicalMapping::Unmapped | PhysicalMapping::Mmio => bytes.as_mut().fill(0xff),
            }
        } else {
            self.read(address, bytes.as_mut())?;
        }
        Ok(V::from_le_bytes(bytes))
    }
}

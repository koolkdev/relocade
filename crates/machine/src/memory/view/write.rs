use super::{page_span, range_fits, segment_address, MemoryError, MemoryView};
use relocade_wasm_runtime::{MemoryAccess, MemoryValue, Store};
use relocade_x86::{PhysicalMapping, SegmentRegister};

impl<'a> MemoryView<'a> {
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

    /// Writes a little-endian 8-bit value with the policy of [`Self::write`].
    #[inline]
    pub fn write_u8(&mut self, address: u32, value: u8) -> Result<(), MemoryError> {
        self.store(address, value)
    }

    /// Writes a little-endian 16-bit value with the policy of [`Self::write`].
    #[inline]
    pub fn write_u16(&mut self, address: u32, value: u16) -> Result<(), MemoryError> {
        self.store(address, value)
    }

    /// Writes a little-endian 32-bit value with the policy of [`Self::write`].
    #[inline]
    pub fn write_u32(&mut self, address: u32, value: u32) -> Result<(), MemoryError> {
        self.store(address, value)
    }

    /// Writes a little-endian 64-bit value with the policy of [`Self::write`].
    #[inline]
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

    #[inline]
    fn store<V: MemoryValue>(&mut self, address: u32, value: V) -> Result<(), MemoryError>
    where
        MemoryAccess<'a>: Store<V>,
    {
        let bytes = value.to_le_bytes();
        let len = bytes.as_ref().len();
        if !range_fits(address, len, 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        if page_span(address, len) == len {
            if let PhysicalMapping::Ram { backing_offset } = self.config.memory_map().get(address) {
                self.storage.store(backing_offset, value);
            }
            Ok(())
        } else {
            self.write(address, bytes.as_ref())
        }
    }
}

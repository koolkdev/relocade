//! Prepared CPU windows and access to their physical backing.
use super::{storage::Storage, VideoMemory};

/// A CPU aperture's origin and physical backing. The operation interprets
/// offsets within this backing as either linear bytes or VGA plane addresses.
/// The card supplies a valid, power-of-two backing extent.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct VramWindow {
    pub cpu_start: u32,
    pub vram_start: u32,
    pub bytes: u32,
}

impl VramWindow {
    /// Bindings bound the transfer; a larger CPU aperture mirrors the backing.
    pub fn offset(self, address: u32) -> u32 {
        (address - self.cpu_start) & (self.bytes - 1)
    }

    pub fn view(self, memory: &mut VideoMemory) -> VramView<'_> {
        VramView {
            storage: &mut memory.storage,
            start: self.vram_start,
        }
    }
}

/// A borrowed view of the backing selected by memory decoding. All addresses
/// are relative to that backing; writes retain physical dirty-page tracking.
pub(crate) struct VramView<'a> {
    storage: &'a mut Storage,
    start: u32,
}

impl VramView<'_> {
    #[inline(always)]
    pub fn read(&self, offset: u32, bytes: u8) -> u64 {
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.storage.read(self.start + offset + index as u32);
        }
        u64::from_le_bytes(value)
    }

    #[inline(always)]
    pub fn write(&mut self, offset: u32, bytes: u8, value: u64) {
        if bytes == 1 {
            self.write_byte(offset, value as u8);
        } else {
            self.write_bytes(offset, &value.to_le_bytes()[..usize::from(bytes)]);
        }
    }

    pub fn read_word(&self, offset: u32) -> u32 {
        self.storage.read_word(self.start + offset)
    }

    pub fn write_byte(&mut self, offset: u32, value: u8) {
        self.storage.write(self.start + offset, value);
    }

    pub fn write_word(&mut self, offset: u32, value: u32) {
        self.storage.write_word(self.start + offset, value);
    }

    pub fn write_masked_word(&mut self, offset: u32, value: u32, mask: u32) {
        self.storage
            .write_masked_word(self.start + offset, value, mask);
    }

    pub fn write_bytes(&mut self, offset: u32, values: &[u8]) {
        self.storage.write_bytes(self.start + offset, values);
    }
}

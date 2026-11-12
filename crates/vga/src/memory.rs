use crate::registers::Registers;

mod access;
pub(crate) use access::{VramAccess, VramWindow};
mod storage;
pub(crate) use storage::MemorySnapshot;
use storage::Storage;

#[cfg(feature = "renderer")]
mod pages;
#[cfg(feature = "renderer")]
pub(crate) use pages::DirtyPages;

pub(crate) const BLOCK_BYTES: usize = 65536;

/// Physical video memory shared by legacy VGA planes and packed SVGA addressing.
pub const VIDEO_MEMORY_BYTES: u32 = 4 * 1024 * 1024;
pub(crate) const BLOCKS: usize = VIDEO_MEMORY_BYTES as usize / BLOCK_BYTES;
pub(crate) const VGA_MEMORY_BYTES: u32 = 4 * BLOCK_BYTES as u32;

/// Shared video memory with VGA access latches. Legacy CPU reads latch all four
/// planes; packed accesses and scanout do not disturb those latches. VGA plane
/// addressing wraps at the hardware's 16-bit limit.
#[derive(Default)]
pub(super) struct VideoMemory {
    storage: Storage,
    latches: [u8; 4],
}

impl VideoMemory {
    /// Native video operations address the same physical bytes as the CPU aperture,
    /// without loading the CPU's read latches or applying its write registers.
    pub fn read_physical(&self, address: u32) -> u8 {
        self.storage.read(address)
    }

    pub fn write_physical(&mut self, address: u32, value: u8) {
        self.storage.write(address, value);
    }

    pub fn latches(&self) -> [u8; 4] {
        self.latches
    }

    pub fn restore_latches(&mut self, latches: [u8; 4]) {
        self.latches = latches;
    }

    /// Reads each byte through the selected aperture; unmapped bytes return FF.
    pub fn read(&mut self, address: u32, bytes: u8, access: VramAccess<'_>) -> u64 {
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            let address = address + index as u32;
            *byte = match access {
                VramAccess::Disabled => 0xff,
                VramAccess::Vga(registers) => self.read_vga_byte(address, registers),
                VramAccess::Window(window) => window
                    .memory_offset(address)
                    .map_or(0xff, |offset| self.storage.read(offset)),
            };
        }
        u64::from_le_bytes(value)
    }

    pub fn write(&mut self, address: u32, bytes: u8, value: u64, access: VramAccess<'_>) {
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            let address = address + index as u32;
            match access {
                VramAccess::Disabled => {}
                VramAccess::Vga(registers) => self.write_vga_byte(address, *byte, registers),
                VramAccess::Window(window) => {
                    if let Some(offset) = window.memory_offset(address) {
                        self.storage.write(offset, *byte);
                    }
                }
            }
        }
    }

    /// Clears all physical VRAM, preserving the VGA CPU read latches.
    pub fn clear_vram(&mut self) {
        self.storage.clear();
    }

    pub fn snapshot(&self, bytes: u32) -> MemorySnapshot {
        self.storage.snapshot(bytes)
    }

    #[cfg(feature = "renderer")]
    pub fn changed_pages(&self) -> DirtyPages {
        self.storage.changed_pages()
    }

    #[cfg(feature = "renderer")]
    pub fn clear_changes(&mut self) {
        self.storage.clear_changes();
    }

    fn index(offset: u32, registers: &Registers) -> usize {
        let address_bits = if registers.sequencer.extended_memory() {
            16
        } else {
            14
        };
        let offset = if registers.sequencer.chain_four() {
            // Chain-4 uses CPU A0/A1 for plane selection. The plane address
            // receives A14/A15 on those pins, matching CRTC doubleword output.
            (offset & !3) | ((offset >> 14) & 3)
        } else if registers.graphics.chain_odd_even() {
            // Only the 128-KiB aperture exposes A16. The IBM VGA target has
            // no programmable page-select input for the smaller apertures.
            (offset & !1) | ((offset >> 16) & 1)
        } else {
            offset
        };
        (offset & ((1 << address_bits) - 1)) as usize
    }

    fn read_vga_byte(&mut self, address: u32, registers: &Registers) -> u8 {
        let graphics = &registers.graphics;
        let Some(offset) = graphics.memory_offset(address) else {
            return 0xff;
        };
        let index = Self::index(offset, registers);
        self.latches = std::array::from_fn(|plane| self.storage.read((index * 4 + plane) as u32));
        if graphics.compare_reads() {
            return graphics.compare(self.latches);
        }
        let plane = if registers.sequencer.chain_four() {
            (offset & 3) as usize
        } else if graphics.odd_even_reads() {
            (graphics.read_plane() & 2) | (offset & 1) as usize
        } else {
            graphics.read_plane()
        };
        self.latches[plane]
    }

    fn write_vga_byte(&mut self, address: u32, value: u8, registers: &Registers) {
        let Some(offset) = registers.graphics.memory_offset(address) else {
            return;
        };
        let index = Self::index(offset, registers);
        let planes = registers.sequencer.write_planes(offset);
        let values = registers.graphics.write_data(value, self.latches);
        for (plane, value) in values.into_iter().enumerate() {
            if planes & (1 << plane) != 0 {
                self.storage.write((index * 4 + plane) as u32, value);
            }
        }
    }
}

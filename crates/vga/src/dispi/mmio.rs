//! DISPI selects VGA memory decoding or the packed-mode banked window.
use crate::{
    memory::VideoMemory,
    mmio::{MemoryDecoding, VgaMemory},
    DispiAdapter, VgaCore,
};

impl DispiAdapter {
    pub(super) fn vga_window_decoding(&self) -> MemoryDecoding {
        if !self.vga.decodes_video_memory() {
            MemoryDecoding::Disabled
        } else if let Some(window) = self.dispi.banked_window() {
            MemoryDecoding::linear(window)
        } else {
            MemoryDecoding::vga(&self.vga)
        }
    }
}

impl VgaMemory for DispiAdapter {
    fn vga_memory(&mut self) -> (&mut VgaCore, &mut VideoMemory) {
        (&mut self.vga, &mut self.memory)
    }
}

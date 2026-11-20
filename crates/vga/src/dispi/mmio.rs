//! DISPI exposes VRAM through the banked window and the linear framebuffer.
use crate::{
    memory::{VideoMemory, VramWindow},
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

    pub(super) fn linear_framebuffer_decoding(&self) -> MemoryDecoding {
        if self.vga.decodes_video_memory() && self.dispi.display_layout().is_some() {
            MemoryDecoding::linear(VramWindow {
                cpu_start: super::LINEAR_FRAMEBUFFER_START,
                vram_start: 0,
                bytes: self.memory.capacity(),
            })
        } else {
            MemoryDecoding::Disabled
        }
    }
}

impl VgaMemory for DispiAdapter {
    fn vga_memory(&mut self) -> (&mut VgaCore, &mut VideoMemory) {
        (&mut self.vga, &mut self.memory)
    }
}

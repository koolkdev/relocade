//! DISPI's banked memory operations and card-level access selection.
use crate::{
    memory::VideoMemory,
    mmio::{MemoryDecoding, VgaMemory},
    DispiAdapter, VgaCore,
};

impl DispiAdapter {
    pub(super) fn memory_decoding(&self) -> MemoryDecoding {
        if !self.vga.setup.enabled() || !self.vga.registers.video_memory_enabled() {
            MemoryDecoding::Disabled
        } else if let Some(window) = self.dispi.bank_mapping() {
            MemoryDecoding::Linear {
                window: window.cpu_start..window.cpu_start + window.bytes,
                read: window,
                write: window,
            }
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

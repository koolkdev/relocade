//! Physical MMIO routing selected by the complete card.
use super::{output, DispiAdapter};
use crate::memory::VramWindow;
use relocade_machine::{Device, IoContext};
use std::{cell::RefCell, rc::Rc};

enum MemoryAccess {
    Disabled,
    Vga,
    Packed(VramWindow),
}

impl DispiAdapter {
    fn memory_access(&self) -> MemoryAccess {
        if !self.vga.setup.enabled() || !self.vga.registers.video_memory_enabled() {
            MemoryAccess::Disabled
        } else if let Some(window) = self.dispi.bank_mapping() {
            MemoryAccess::Packed(window)
        } else {
            MemoryAccess::Vga
        }
    }
}

impl Device for DispiAdapter {
    async fn run(device: Rc<RefCell<Self>>) {
        output::run(device).await;
    }
    fn read_mmio(&mut self, _: IoContext<'_>, address: u32, bytes: u8) -> u64 {
        match self.memory_access() {
            MemoryAccess::Disabled => u64::MAX >> (64 - u32::from(bytes) * 8),
            MemoryAccess::Vga => self.vga.read_memory(&self.memory, address, bytes),
            MemoryAccess::Packed(window) => self.memory.read(address, bytes, window),
        }
    }
    fn write_mmio(&mut self, _: IoContext<'_>, address: u32, bytes: u8, value: u64) {
        self.capture_if_due();
        match self.memory_access() {
            MemoryAccess::Disabled => {}
            MemoryAccess::Vga => self
                .vga
                .write_memory(&mut self.memory, address, bytes, value),
            MemoryAccess::Packed(window) => self.memory.write(address, bytes, value, window),
        }
    }
}

//! Physical MMIO routing selected by the complete card.
use super::{output, DispiAdapter};
use crate::{memory::VramWindow, MEMORY_RANGE};
use relocade_machine::{Device, DeviceCallbacks, DeviceConnection, MmioRead, MmioWrite};
use std::{cell::RefCell, rc::Rc};

enum MemoryAccess {
    Disabled,
    Vga,
    Packed(VramWindow),
}

impl DispiAdapter {
    /// Connects the memory aperture and individual VGA/DISPI port operations.
    pub fn configure_io(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<Self>>) {
        assert!(device.borrow().io.is_none(), "VGA is already connected");
        let callbacks = DeviceCallbacks::new(device);
        io.bind_mmio(
            MEMORY_RANGE,
            &MmioRead::new(
                callbacks.read_mmio(|_, card, address, bytes| card.read_memory(address, bytes)),
            ),
            &MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
                card.write_memory(address, bytes, value);
            })),
        );
        Self::bind_ports(io, device);
    }

    fn memory_access(&self) -> MemoryAccess {
        if !self.vga.setup.enabled() || !self.vga.registers.video_memory_enabled() {
            MemoryAccess::Disabled
        } else if let Some(window) = self.dispi.bank_mapping() {
            MemoryAccess::Packed(window)
        } else {
            MemoryAccess::Vga
        }
    }
    fn read_memory(&mut self, address: u32, bytes: u8) -> u64 {
        match self.memory_access() {
            MemoryAccess::Disabled => u64::MAX >> (64 - u32::from(bytes) * 8),
            MemoryAccess::Vga => self.vga.read_memory(&self.memory, address, bytes),
            MemoryAccess::Packed(window) => self.memory.read(address, bytes, window),
        }
    }
    fn write_memory(&mut self, address: u32, bytes: u8, value: u64) {
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

impl Device for DispiAdapter {
    async fn run(device: Rc<RefCell<Self>>) {
        output::run(device).await;
    }
}

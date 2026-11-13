//! DISPI's banked memory operations and card-level access selection.
use crate::{
    memory::VideoMemory,
    mmio::{MemoryDecoding, MemoryMapping, VgaMemory},
    DispiAdapter, VgaCore,
};
use relocade_machine::{DeviceCallbacks, DeviceConnection, MmioRead, MmioWrite};
use std::{cell::RefCell, rc::Rc};

impl DispiAdapter {
    pub(super) fn memory_decoding(&self) -> MemoryDecoding {
        if !self.vga.setup.enabled() || !self.vga.registers.video_memory_enabled() {
            MemoryDecoding::Disabled
        } else if let Some(window) = self.dispi.bank_mapping() {
            MemoryDecoding::Packed(window)
        } else {
            MemoryDecoding::Vga {
                window: self.vga.memory_window(),
                write: self.vga.memory_write(),
            }
        }
    }

    #[inline(never)]
    fn read_packed_memory(&mut self, address: u32, bytes: u8) -> u64 {
        self.memory.read(address, bytes, self.packed_window)
    }

    #[inline(never)]
    fn write_packed_memory(&mut self, address: u32, bytes: u8, value: u64) {
        self.memory.write(address, bytes, value, self.packed_window);
    }
}

impl DispiAdapter {
    pub(super) fn bind_memory(
        io: &mut DeviceConnection<'_>,
        device: &Rc<RefCell<Self>>,
    ) -> MemoryMapping {
        let callbacks = DeviceCallbacks::new(device);
        let read = MmioRead::new(
            callbacks.read_mmio(|_, card, address, bytes| card.read_packed_memory(address, bytes)),
        );
        let write = MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
            card.write_packed_memory(address, bytes, value)
        }));
        MemoryMapping::new(io, device, Some((read, write)))
    }
}

impl VgaMemory for DispiAdapter {
    fn vga_memory(&mut self) -> (&mut VgaCore, &mut VideoMemory) {
        (&mut self.vga, &mut self.memory)
    }
}

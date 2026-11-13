//! Physical MMIO routing selected when the complete card is configured.
use super::DispiAdapter;
use crate::{MEMORY_RANGE, memory::VramWindow};
use relocade_machine::{
    DeviceCallbacks, DeviceConnection, IoContext, MmioBinding, MmioRead, MmioWrite,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MemoryDecoding {
    Disabled,
    Vga { direct_writes: bool },
    Packed(VramWindow),
}

pub(super) struct MemoryMapping {
    binding: MmioBinding,
    vga_read: MmioRead,
    vga_write: MmioWrite,
    vga_direct_write: MmioWrite,
    packed_read: MmioRead,
    packed_write: MmioWrite,
    disabled_read: MmioRead,
    disabled_write: MmioWrite,
}

impl DispiAdapter {
    pub(super) fn memory_decoding(&self) -> MemoryDecoding {
        if !self.vga.setup.enabled() || !self.vga.registers.video_memory_enabled() {
            MemoryDecoding::Disabled
        } else if let Some(window) = self.dispi.bank_mapping() {
            MemoryDecoding::Packed(window)
        } else {
            MemoryDecoding::Vga {
                direct_writes: self.vga.direct_memory_writes(),
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

impl MemoryMapping {
    pub(super) fn new(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<DispiAdapter>>) -> Self {
        let callbacks = DeviceCallbacks::new(device);
        let disabled_read = MmioRead::new(|_, _, bytes| u64::MAX >> (64 - u32::from(bytes) * 8));
        let disabled_write = MmioWrite::new(|_, _, _, _| {});
        Self {
            binding: io.bind_mmio(MEMORY_RANGE, &disabled_read, &disabled_write),
            vga_read: MmioRead::new(callbacks.read_mmio(|_, card, address, bytes| {
                card.vga.read_memory(&card.memory, address, bytes)
            })),
            vga_write: MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
                card.vga
                    .write_memory::<false>(&mut card.memory, address, bytes, value)
            })),
            vga_direct_write: MmioWrite::new(callbacks.write_mmio(
                |_, card, address, bytes, value| {
                    card.vga
                        .write_memory::<true>(&mut card.memory, address, bytes, value)
                },
            )),
            packed_read: MmioRead::new(
                callbacks
                    .read_mmio(|_, card, address, bytes| card.read_packed_memory(address, bytes)),
            ),
            packed_write: MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
                card.write_packed_memory(address, bytes, value)
            })),
            disabled_read,
            disabled_write,
        }
    }

    /// Only a change of operation replaces a handler. A bank change updates
    /// the prepared window on the card without replacing either packed handler.
    pub fn update_decoding(
        &self,
        mut context: IoContext<'_>,
        old: Option<MemoryDecoding>,
        decoding: MemoryDecoding,
    ) {
        match decoding {
            MemoryDecoding::Disabled => {
                self.binding
                    .set_read(context.reborrow(), &self.disabled_read);
                self.binding.set_write(context, &self.disabled_write);
            }
            MemoryDecoding::Vga { direct_writes } => {
                if !matches!(old, Some(MemoryDecoding::Vga { .. })) {
                    self.binding.set_read(context.reborrow(), &self.vga_read);
                }
                self.binding.set_write(
                    context,
                    if direct_writes {
                        &self.vga_direct_write
                    } else {
                        &self.vga_write
                    },
                );
            }
            MemoryDecoding::Packed(_) => {
                if !matches!(old, Some(MemoryDecoding::Packed(_))) {
                    self.binding.set_read(context.reborrow(), &self.packed_read);
                    self.binding.set_write(context, &self.packed_write);
                }
            }
        }
    }
}

//! Physical MMIO routing selected when the complete card is configured.
use super::DispiAdapter;
use crate::{core::MemoryWrite, memory::VramWindow};
use relocade_machine::{
    DeviceCallbacks, DeviceConnection, IoContext, MmioBinding, MmioRead, MmioWrite,
};
use std::{
    cell::RefCell,
    ops::{Range, RangeInclusive},
    rc::Rc,
};

// Every VGA aperture is a union of these fixed, page-aligned regions.
const REGIONS: [RangeInclusive<u32>; 3] = [0xa0000..=0xaffff, 0xb0000..=0xb7fff, 0xb8000..=0xbffff];

#[derive(Clone, PartialEq, Eq)]
pub(super) enum MemoryDecoding {
    Disabled,
    Vga {
        window: Range<u32>,
        write: MemoryWrite,
    },
    Packed(VramWindow),
}

pub(super) struct MemoryMapping {
    bindings: [MmioBinding; 3],
    vga_read: MmioRead,
    vga_write: MmioWrite,
    vga_copy_write: MmioWrite,
    chain4_write: MmioWrite,
    all_planes_write: MmioWrite,
    one_plane_write: MmioWrite,
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

impl MemoryMapping {
    pub(super) fn new(io: &mut DeviceConnection<'_>, device: &Rc<RefCell<DispiAdapter>>) -> Self {
        let callbacks = DeviceCallbacks::new(device);
        let disabled_read = MmioRead::new(|_, _, bytes| u64::MAX >> (64 - u32::from(bytes) * 8));
        let disabled_write = MmioWrite::new(|_, _, _, _| {});
        Self {
            bindings: REGIONS.map(|region| io.bind_mmio(region, &disabled_read, &disabled_write)),
            vga_read: MmioRead::new(callbacks.read_mmio(|_, card, address, bytes| {
                card.vga.read_memory(&card.memory, address, bytes)
            })),
            vga_write: MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
                card.vga
                    .write_memory::<false>(&mut card.memory, address, bytes, value)
            })),
            vga_copy_write: MmioWrite::new(callbacks.write_mmio(
                |_, card, address, bytes, value| {
                    card.vga
                        .write_memory::<true>(&mut card.memory, address, bytes, value)
                },
            )),
            chain4_write: MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
                card.vga
                    .write_chain4(&mut card.memory, address, bytes, value)
            })),
            all_planes_write: MmioWrite::new(callbacks.write_mmio(
                |_, card, address, bytes, value| {
                    card.vga
                        .write_all_planes(&mut card.memory, address, bytes, value)
                },
            )),
            one_plane_write: MmioWrite::new(callbacks.write_mmio(
                |_, card, address, bytes, value| {
                    card.vga
                        .write_one_plane(&mut card.memory, address, bytes, value)
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

    fn operations(&self, decoding: &MemoryDecoding, address: u32) -> (&MmioRead, &MmioWrite) {
        match decoding {
            MemoryDecoding::Vga { window, write } if window.contains(&address) => {
                let write = match write {
                    MemoryWrite::Disabled => &self.disabled_write,
                    MemoryWrite::Graphics => &self.vga_write,
                    MemoryWrite::MaskedCopy => &self.vga_copy_write,
                    MemoryWrite::Chain4 => &self.chain4_write,
                    MemoryWrite::AllPlanes => &self.all_planes_write,
                    MemoryWrite::OnePlane => &self.one_plane_write,
                };
                (&self.vga_read, write)
            }
            MemoryDecoding::Packed(window)
                if (window.cpu_start..window.cpu_start + window.bytes).contains(&address) =>
            {
                (&self.packed_read, &self.packed_write)
            }
            _ => (&self.disabled_read, &self.disabled_write),
        }
    }

    /// Bindings enforce the active aperture. Reinstall only operations that
    /// change; a bank or single-plane selection updates prepared card state.
    pub fn update_decoding(
        &self,
        mut context: IoContext<'_>,
        old: Option<MemoryDecoding>,
        decoding: MemoryDecoding,
    ) {
        for (binding, region) in self.bindings.iter().zip(REGIONS) {
            let (read, write) = self.operations(&decoding, *region.start());
            let previous = old
                .as_ref()
                .map(|old| self.operations(old, *region.start()));
            if previous.is_none_or(|(previous, _)| !std::ptr::eq(previous, read)) {
                binding.set_read(context.reborrow(), read);
            }
            if previous.is_none_or(|(_, previous)| !std::ptr::eq(previous, write)) {
                binding.set_write(context.reborrow(), write);
            }
        }
    }
}

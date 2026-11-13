//! Physical MMIO routing selected when the complete card is configured.
use crate::{core::MemoryWrite, memory::VramWindow};
use crate::{memory::VideoMemory, VgaCore};
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
pub(crate) enum MemoryDecoding {
    Disabled,
    Vga {
        window: Range<u32>,
        write: MemoryWrite,
    },
    Packed(VramWindow),
}

/// Borrow the independent core and backing together for a concrete memory callback.
pub(crate) trait VgaMemory {
    fn vga_memory(&mut self) -> (&mut VgaCore, &mut VideoMemory);
}

impl MemoryDecoding {
    pub fn vga(vga: &VgaCore) -> Self {
        if !vga.setup.enabled() || !vga.registers.video_memory_enabled() {
            Self::Disabled
        } else {
            Self::Vga {
                window: vga.memory_window(),
                write: vga.memory_write(),
            }
        }
    }
}

pub(crate) struct MemoryMapping {
    bindings: [MmioBinding; 3],
    vga_read: MmioRead,
    vga_write: MmioWrite,
    vga_copy_write: MmioWrite,
    chain4_write: MmioWrite,
    one_plane_write: MmioWrite,
    packed: Option<(MmioRead, MmioWrite)>,
    disabled_read: MmioRead,
    disabled_write: MmioWrite,
}

impl MemoryMapping {
    pub(crate) fn new<Card: VgaMemory + 'static>(
        io: &mut DeviceConnection<'_>,
        device: &Rc<RefCell<Card>>,
        packed: Option<(MmioRead, MmioWrite)>,
    ) -> Self {
        let callbacks = DeviceCallbacks::new(device);
        let disabled_read = MmioRead::new(|_, _, bytes| u64::MAX >> (64 - u32::from(bytes) * 8));
        let disabled_write = MmioWrite::new(|_, _, _, _| {});
        Self {
            bindings: REGIONS.map(|region| io.bind_mmio(region, &disabled_read, &disabled_write)),
            vga_read: MmioRead::new(callbacks.read_mmio(|_, card, address, bytes| {
                let (vga, memory) = card.vga_memory();
                vga.read_memory(memory, address, bytes)
            })),
            vga_write: MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
                let (vga, memory) = card.vga_memory();
                vga.write_memory::<false>(memory, address, bytes, value)
            })),
            vga_copy_write: MmioWrite::new(callbacks.write_mmio(
                |_, card, address, bytes, value| {
                    let (vga, memory) = card.vga_memory();
                    vga.write_memory::<true>(memory, address, bytes, value)
                },
            )),
            chain4_write: MmioWrite::new(callbacks.write_mmio(|_, card, address, bytes, value| {
                let (vga, memory) = card.vga_memory();
                vga.write_chain4(memory, address, bytes, value)
            })),
            one_plane_write: MmioWrite::new(callbacks.write_mmio(
                |_, card, address, bytes, value| {
                    let (vga, memory) = card.vga_memory();
                    vga.write_one_plane(memory, address, bytes, value)
                },
            )),
            packed,
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
                    MemoryWrite::OnePlane => &self.one_plane_write,
                };
                (&self.vga_read, write)
            }
            MemoryDecoding::Packed(window)
                if (window.cpu_start..window.cpu_start + window.bytes).contains(&address) =>
            {
                let (read, write) = self.packed.as_ref().expect("connected packed memory");
                (read, write)
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

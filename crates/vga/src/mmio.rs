//! Physical MMIO routing selected when the complete card is configured.
use crate::{
    core::MemoryWrite,
    memory::{VideoMemory, VramView, VramWindow, VGA_MEMORY_BYTES},
    VgaCore,
};
use relocade_machine::{
    DeviceCallbacks, DeviceConnection, IoContext, MmioBinding, MmioRead, MmioWrite,
};
use std::{
    cell::{Cell, RefCell},
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
        write_operation: MemoryWrite,
        read: VramWindow,
        write: VramWindow,
    },
    Linear {
        window: Range<u32>,
        read: VramWindow,
        write: VramWindow,
    },
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
            let window = vga.memory_window();
            let backing = VramWindow {
                cpu_start: window.start,
                vram_start: 0,
                bytes: VGA_MEMORY_BYTES,
            };
            Self::Vga {
                window,
                write_operation: vga.memory_write(),
                read: backing,
                write: backing,
            }
        }
    }
}

pub(crate) struct MemoryMapping {
    bindings: [MmioBinding; 3],
    read_window: Rc<Cell<VramWindow>>,
    write_window: Rc<Cell<VramWindow>>,
    vga_read: MmioRead,
    vga_write: MmioWrite,
    vga_copy_write: MmioWrite,
    chain4_write: MmioWrite,
    one_plane_write: MmioWrite,
    linear_read: MmioRead,
    linear_write: MmioWrite,
    disabled_read: MmioRead,
    disabled_write: MmioWrite,
}

impl MemoryMapping {
    pub(crate) fn new<Card: VgaMemory + 'static>(
        io: &mut DeviceConnection<'_>,
        device: &Rc<RefCell<Card>>,
    ) -> Self {
        let callbacks = DeviceCallbacks::new(device);
        let disabled_read = MmioRead::new(|_, _, bytes| u64::MAX >> (64 - u32::from(bytes) * 8));
        let disabled_write = MmioWrite::new(|_, _, _, _| {});
        // Only disabled operations are installed until update_decoding supplies
        // the windows. Every prepared operation follows those same live views.
        let read_window = Rc::new(Cell::new(VramWindow::default()));
        let write_window = Rc::new(Cell::new(VramWindow::default()));
        Self {
            bindings: REGIONS.map(|region| io.bind_mmio(region, &disabled_read, &disabled_write)),
            vga_read: read_operation(&callbacks, &read_window, VgaCore::read_memory),
            vga_write: write_operation(&callbacks, &write_window, VgaCore::write_memory::<false>),
            vga_copy_write: write_operation(
                &callbacks,
                &write_window,
                VgaCore::write_memory::<true>,
            ),
            chain4_write: write_operation(&callbacks, &write_window, VgaCore::write_chain4),
            one_plane_write: write_operation(&callbacks, &write_window, VgaCore::write_one_plane),
            linear_read: read_operation(&callbacks, &read_window, |_, memory, offset, bytes| {
                memory.read(offset, bytes)
            }),
            linear_write: write_operation(
                &callbacks,
                &write_window,
                |_, mut memory, offset, bytes, value| memory.write(offset, bytes, value),
            ),
            read_window,
            write_window,
            disabled_read,
            disabled_write,
        }
    }

    fn operations(&self, decoding: &MemoryDecoding, address: u32) -> (&MmioRead, &MmioWrite) {
        match decoding {
            MemoryDecoding::Vga {
                window,
                write_operation,
                ..
            } if window.contains(&address) => {
                let write = match write_operation {
                    MemoryWrite::Disabled => &self.disabled_write,
                    MemoryWrite::Graphics => &self.vga_write,
                    MemoryWrite::MaskedCopy => &self.vga_copy_write,
                    MemoryWrite::Chain4 => &self.chain4_write,
                    MemoryWrite::OnePlane => &self.one_plane_write,
                };
                (&self.vga_read, write)
            }
            MemoryDecoding::Linear { window, .. } if window.contains(&address) => {
                (&self.linear_read, &self.linear_write)
            }
            _ => (&self.disabled_read, &self.disabled_write),
        }
    }

    /// Bindings enforce the active aperture. Reinstall only operations that
    /// change; a bank switch updates the backing used by retained operations.
    pub fn update_decoding(
        &self,
        mut context: IoContext<'_>,
        old: Option<MemoryDecoding>,
        decoding: MemoryDecoding,
    ) {
        if let MemoryDecoding::Vga { read, write, .. }
        | MemoryDecoding::Linear { read, write, .. } = &decoding
        {
            self.read_window.set(*read);
            self.write_window.set(*write);
        }
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

fn read_operation<Card: VgaMemory + 'static>(
    callbacks: &DeviceCallbacks<Card>,
    window: &Rc<Cell<VramWindow>>,
    read: impl Fn(&mut VgaCore, VramView<'_>, u32, u8) -> u64 + 'static,
) -> MmioRead {
    let window = window.clone();
    MmioRead::new(callbacks.read_mmio(move |_, card, address, bytes| {
        let window = window.get();
        let (vga, memory) = card.vga_memory();
        read(vga, window.view(memory), window.offset(address), bytes)
    }))
}

fn write_operation<Card: VgaMemory + 'static>(
    callbacks: &DeviceCallbacks<Card>,
    window: &Rc<Cell<VramWindow>>,
    write: impl Fn(&VgaCore, VramView<'_>, u32, u8, u64) + 'static,
) -> MmioWrite {
    let window = window.clone();
    MmioWrite::new(callbacks.write_mmio(move |_, card, address, bytes, value| {
        let window = window.get();
        let (vga, memory) = card.vga_memory();
        write(
            vga,
            window.view(memory),
            window.offset(address),
            bytes,
            value,
        );
    }))
}

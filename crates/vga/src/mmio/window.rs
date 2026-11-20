//! MMIO handlers and VRAM views for one CPU-visible memory window.
use super::{MemoryDecoding, VgaMemory};
use crate::{
    core::MemoryWrite,
    memory::{VramView, VramWindow},
    VgaCore,
};
use relocade_machine::{
    DeviceCallbacks, DeviceConnection, IoContext, MmioBinding, MmioRead, MmioWrite,
};
use std::{
    cell::{Cell, RefCell},
    ops::RangeInclusive,
    rc::Rc,
};

pub(crate) struct MmioWindow<Card> {
    bindings: Vec<(u32, MmioBinding)>,
    decode: fn(&Card) -> MemoryDecoding,
    installed: RefCell<MemoryDecoding>,
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

impl<Card> MmioWindow<Card> {
    pub(crate) fn bind(
        io: &mut DeviceConnection<'_>,
        device: &Rc<RefCell<Card>>,
        regions: impl IntoIterator<Item = RangeInclusive<u32>>,
        decode: fn(&Card) -> MemoryDecoding,
    ) -> Self
    where
        Card: VgaMemory + 'static,
    {
        let callbacks = DeviceCallbacks::new(device);
        let disabled_read = MmioRead::new(|_, _, bytes| u64::MAX >> (64 - u32::from(bytes) * 8));
        let disabled_write = MmioWrite::new(|_, _, _, _| {});
        // Only disabled operations are installed until update_decoding supplies
        // the windows. Every prepared operation follows those same live views.
        let read_window = Rc::new(Cell::new(VramWindow::default()));
        let write_window = Rc::new(Cell::new(VramWindow::default()));
        let mapping = Self {
            decode,
            installed: RefCell::new(MemoryDecoding::Disabled),
            bindings: regions
                .into_iter()
                .map(|region| {
                    (
                        *region.start(),
                        io.bind_mmio(region, &disabled_read, &disabled_write),
                    )
                })
                .collect(),
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
        };
        mapping.update_decoding(io.context(), &device.borrow());
        mapping
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

    /// A bank switch can move the VRAM backing while the same handlers stay installed.
    pub(crate) fn update_decoding(&self, mut context: IoContext<'_>, card: &Card) {
        let decoding = (self.decode)(card);
        let mut installed = self.installed.borrow_mut();
        if *installed == decoding {
            return;
        }
        if let MemoryDecoding::Vga { read, write, .. }
        | MemoryDecoding::Linear { read, write, .. } = &decoding
        {
            self.read_window.set(*read);
            self.write_window.set(*write);
        }
        for (address, binding) in &self.bindings {
            let (read, write) = self.operations(&decoding, *address);
            let (old_read, old_write) = self.operations(&installed, *address);
            if !std::ptr::eq(old_read, read) {
                binding.set_read(context.reborrow(), read);
            }
            if !std::ptr::eq(old_write, write) {
                binding.set_write(context.reborrow(), write);
            }
        }
        *installed = decoding;
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

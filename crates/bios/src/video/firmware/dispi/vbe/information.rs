//! DISPI capabilities supplied to the shared VBE information buffers.
use super::{available, layout, Mode};
use crate::video::firmware::vbe::{self, WindowAccess};
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};
use relocade_vga::DispiAdapter;
use std::cell::RefCell;

pub(in super::super) fn initialize(memory: &mut MemoryView<'_>) {
    vbe::initialize(
        memory,
        b"Relocade banked SVGA\0",
        Mode::ALL.map(|mode| mode.number),
    );
}

#[interrupt(context(memory), args(destination = ptr(es, di)),
    returns { true => { ax = 0x004f }, false => { ax = 0x014f } },
)]
pub(super) fn controller(
    memory: &mut MemoryView<'_>,
    card: &RefCell<DispiAdapter>,
    destination: FarPointer,
) -> bool {
    if !available(card) {
        return false;
    }
    let memory_bytes = card.borrow().video_memory_bytes();
    vbe::controller(memory, destination, memory_bytes);
    true
}

#[interrupt(context(memory), args(number = cx, destination = ptr(es, di)),
    returns { true => { ax = 0x004f }, false => { ax = 0x014f } },
)]
pub(super) fn mode(
    memory: &mut MemoryView<'_>,
    card: &RefCell<DispiAdapter>,
    number: u16,
    destination: FarPointer,
) -> bool {
    let Some(mode) = Mode::from_number(number).filter(|_| available(card)) else {
        return false;
    };
    let layout = layout::active(card)
        .filter(|layout| {
            layout.size() == mode.display.size() && layout.format() == mode.display.format()
        })
        .unwrap_or_else(|| {
            mode.display
                .layout(card.borrow().video_memory_bytes())
                .expect("supported mode fits VRAM")
        });
    vbe::banked_mode(
        memory,
        destination,
        layout,
        [WindowAccess::ReadWrite, WindowAccess::Absent],
    );
    true
}

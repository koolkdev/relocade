//! Validated mode and independent bank services for the ET4000 board.
use super::super::modes::Mode;
use crate::video::modes as legacy;
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::Et4000Ax;
use std::cell::RefCell;

pub(super) fn set(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    card: &RefCell<Et4000Ax>,
    number: u16,
    preserve: bool,
) -> bool {
    let Some(mode) = Mode::from_vbe_number(number) else {
        // Includes unsupported linear-framebuffer requests and reserved flags.
        return false;
    };
    if mode.layout(card.borrow().video_memory_bytes()).is_none() {
        return false;
    }
    mode.activate(memory, io, card, preserve);
    true
}

pub(super) fn current(memory: &MemoryView<'_>, card: &Et4000Ax) -> Option<u16> {
    if !card.ports_enabled() {
        return None;
    }
    if let Some(layout) = card.packed_layout() {
        Mode::ALL
            .iter()
            .find(|mode| {
                mode.size.map(u32::from) == layout.size() && mode.format == layout.format()
            })
            .map(|mode| mode.vbe_number)
    } else {
        // VBE 1.2 returns the mode without the memory-preservation flag.
        let number = legacy::mode(memory).mode & 0x7f;
        Mode::from_bios_number(number)
            .is_none()
            .then_some(u16::from(number))
    }
}

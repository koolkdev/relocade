//! Validated mode and independent bank services for the ET4000 board.
use super::super::modes::Mode;
use crate::video::modes as legacy;
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::Et4000Ax;
use std::cell::RefCell;

pub(in crate::video::firmware) fn set(
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

pub(in crate::video::firmware) fn current(
    memory: &MemoryView<'_>,
    card: &RefCell<Et4000Ax>,
) -> Option<u16> {
    let card = card.borrow();
    if !card.ports_enabled() {
        return None;
    }
    if let Some(layout) = card.packed_layout() {
        Mode::ALL
            .iter()
            .find(|mode| mode.size.map(u32::from) == layout.size())
            .map(|mode| mode.vbe_number)
    } else {
        // VBE 1.2 returns the mode without the memory-preservation flag.
        let number = legacy::mode(memory).mode & 0x7f;
        Mode::from_bios_number(number)
            .is_none()
            .then_some(u16::from(number))
    }
}

pub(in crate::video::firmware) fn window(
    io: &IoBus,
    card: &RefCell<Et4000Ax>,
    operation: u16,
    position: u16,
) -> Option<u16> {
    let mut card = card.borrow_mut();
    if !card.ports_enabled() {
        return None;
    }
    card.packed_layout()?;
    match operation {
        // Window A is writable; window B is readable. Neither changes the other.
        0 => io
            .with_context(|context| card.set_write_bank(context, position))
            .then_some(position),
        1 => io
            .with_context(|context| card.set_read_bank(context, position))
            .then_some(position),
        0x100 => Some(card.write_bank()),
        0x101 => Some(card.read_bank()),
        _ => None,
    }
}

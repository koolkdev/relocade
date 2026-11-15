//! VBE mode and bank requests applied through the selected card's driver.
use super::Vbe;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};
use std::cell::RefCell;

#[interrupt(context(memory, io), args(request = bx),
    returns { true => { ax = 0x004f }, false => { ax = 0x014f } },
)]
pub(super) fn set<C: Vbe>(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    card: &RefCell<C>,
    request: u16,
) -> bool {
    if !card.borrow().ports_enabled() {
        return false;
    }
    C::set_mode(card, memory, io, request & 0x7fff, request & 0x8000 != 0)
}

#[interrupt(context(memory), returns {
    Some(number) => { ax = 0x004f, bx = number }, None => { ax = 0x014f },
})]
pub(super) fn current<C: Vbe>(memory: &MemoryView<'_>, card: &RefCell<C>) -> Option<u16> {
    card.borrow().current_mode(memory)
}

#[interrupt(context(io), args(operation = bx, position = dx), returns {
    Some(position) => { ax = 0x004f, dx = position }, None => { ax = 0x014f },
})]
pub(super) fn window<C: Vbe>(
    io: &IoBus,
    card: &RefCell<C>,
    operation: u16,
    position: u16,
) -> Option<u16> {
    let [window, operation] = operation.to_le_bytes();
    let bank = match operation {
        0 => Some(position),
        1 => None,
        _ => return None,
    };
    let mut card = card.borrow_mut();
    card.ports_enabled().then_some(())?;
    card.packed_layout()?;
    io.with_context(|context| card.window(context, window, bank))
}

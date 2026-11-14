//! VBE mode and bank requests applied through the selected card's firmware.
use super::super::{dispi, et4000, VideoFirmware};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};

#[interrupt(context(memory, io), args(request = bx),
    returns { true => { ax = 0x004f }, false => { ax = 0x014f } },
)]
pub(super) fn set(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    firmware: &VideoFirmware,
    request: u16,
) -> bool {
    if !firmware.ports_enabled() {
        return false;
    }
    let number = request & 0x7fff;
    let preserve = request & 0x8000 != 0;
    if number < 0x80 {
        return firmware
            .set_mode(memory, io, number as u8 | if preserve { 0x80 } else { 0 })
            .is_some();
    }
    match firmware {
        VideoFirmware::Vga => false,
        VideoFirmware::Dispi(card) => dispi::vbe::modes::set(memory, io, card, number, preserve),
        VideoFirmware::Et4000Ax(card) => {
            et4000::vbe::modes::set(memory, io, card, number, preserve)
        }
    }
}

#[interrupt(context(memory), returns {
    Some(number) => { ax = 0x004f, bx = number }, None => { ax = 0x014f },
})]
pub(super) fn current(memory: &MemoryView<'_>, firmware: &VideoFirmware) -> Option<u16> {
    match firmware {
        VideoFirmware::Vga => None,
        VideoFirmware::Dispi(card) => dispi::vbe::modes::current(memory, card),
        VideoFirmware::Et4000Ax(card) => et4000::vbe::modes::current(memory, card),
    }
}

#[interrupt(context(io), args(operation = bx, position = dx), returns {
    Some(position) => { ax = 0x004f, dx = position }, None => { ax = 0x014f },
})]
pub(super) fn window(
    io: &IoBus,
    firmware: &VideoFirmware,
    operation: u16,
    position: u16,
) -> Option<u16> {
    match firmware {
        VideoFirmware::Vga => None,
        VideoFirmware::Dispi(card) => dispi::vbe::modes::window(io, card, operation, position),
        VideoFirmware::Et4000Ax(card) => et4000::vbe::modes::window(io, card, operation, position),
    }
}

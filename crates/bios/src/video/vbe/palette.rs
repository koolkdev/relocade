//! DAC format control; color transfers use the existing VGA palette services.
use super::available;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::registers::dac::Dac;

#[interrupt(context(io), args(operation = bl, bits = bh), returns {
    Some(bits) => { ax = 0x004f, bh = bits }, None => { ax = 0x014f },
})]
pub(super) fn format(io: &IoBus, operation: u8, bits: u8) -> Option<u8> {
    if !available(io) {
        return None;
    }
    // VBE 2.0 section 4.11 specifies the next lower supported width when the
    // requested width is unavailable. Our fixed DAC cannot select below six.
    // https://www.phatcode.net/res/221/files/vbe20.pdf
    match operation {
        0 if bits >= Dac::BITS_PER_COMPONENT => Some(Dac::BITS_PER_COMPONENT),
        1 => Some(Dac::BITS_PER_COMPONENT),
        _ => None,
    }
}

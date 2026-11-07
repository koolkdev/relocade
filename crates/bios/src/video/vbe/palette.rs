//! DAC format control; color transfers use the existing VGA palette services.
use super::{available, Mode};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::{registers::dac::Dac, PixelFormat};

pub(super) enum FormatError {
    Failed,
    DirectColor,
}

#[interrupt(context(io), args(operation = bl, bits = bh), returns {
    Ok(bits) => { ax = 0x004f, bh = bits },
    Err(FormatError::Failed) => { ax = 0x014f },
    Err(FormatError::DirectColor) => { ax = 0x034f },
})]
pub(super) fn format(io: &IoBus, operation: u8, bits: u8) -> Result<u8, FormatError> {
    if !available(io) {
        return Err(FormatError::Failed);
    }
    // VBE 2.0 section 4.11 makes DAC-format control invalid in direct-color modes.
    if Mode::active(io).is_some_and(|mode| mode.display.format() != PixelFormat::Indexed8) {
        return Err(FormatError::DirectColor);
    }
    // VBE 2.0 section 4.11 specifies the next lower supported width when the
    // requested width is unavailable. Our fixed DAC cannot select below six.
    // https://www.phatcode.net/res/221/files/vbe20.pdf
    match operation {
        0 if bits >= Dac::BITS_PER_COMPONENT => Ok(Dac::BITS_PER_COMPONENT),
        1 => Ok(Dac::BITS_PER_COMPONENT),
        _ => Err(FormatError::Failed),
    }
}

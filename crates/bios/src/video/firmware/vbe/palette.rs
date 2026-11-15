//! DAC format control; color transfers use the existing VGA palette services.
use super::Vbe;
use relocade_machine::real_mode_interrupt::interrupt;
use relocade_vga::PixelFormat;
use std::cell::RefCell;

pub(in crate::video::firmware) enum FormatError {
    Failed,
    DirectColor,
}

#[interrupt(args(operation = bl, bits = bh), returns {
    Ok(bits) => { ax = 0x004f, bh = bits },
    Err(FormatError::Failed) => { ax = 0x014f },
    Err(FormatError::DirectColor) => { ax = 0x034f },
})]
pub(in crate::video::firmware) fn format<C: Vbe>(
    card: &RefCell<C>,
    operation: u8,
    bits: u8,
) -> Result<u8, FormatError> {
    let card = card.borrow();
    if !card.ports_enabled() {
        return Err(FormatError::Failed);
    }
    // VBE 2.0 section 4.11 makes DAC-format control invalid in direct-color modes.
    if card
        .packed_layout()
        .is_some_and(|layout| layout.format() != PixelFormat::Indexed8)
    {
        return Err(FormatError::DirectColor);
    }
    // VBE 2.0 section 4.11 specifies the next lower supported width when the
    // requested width is unavailable. Each driver supplies its fixed DAC width.
    // https://www.phatcode.net/res/221/files/vbe20.pdf
    match operation {
        0 if bits >= C::DAC_BITS_PER_COMPONENT => Ok(C::DAC_BITS_PER_COMPONENT),
        1 => Ok(C::DAC_BITS_PER_COMPONENT),
        _ => Err(FormatError::Failed),
    }
}

//! Logical framebuffer services use the device's checked layout through DISPI.
//! VBE 2.0 sections 4.9–4.10 define the extended selectors and error results:
//! https://www.phatcode.net/res/221/files/vbe20.pdf
use super::{available, read_register, write_register, Mode};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::{registers::dispi, SvgaLayout};

pub(super) fn active(io: &IoBus) -> Option<SvgaLayout> {
    if !available(io) {
        return None;
    }
    Mode::active(io)?
        .display
        .layout()
        .with_pitch(u32::from(read_register(io, dispi::VIRTUAL_WIDTH)))?
        .with_origin([
            u32::from(read_register(io, dispi::X_OFFSET)),
            u32::from(read_register(io, dispi::Y_OFFSET)),
        ])
}

pub(super) enum ScanlineError {
    Failed,
    TooWide,
}

#[interrupt(context(io), args(operation = bl, width = cx), returns {
    Ok(layout) => {
        ax = 0x004f,
        bx = layout.pitch() as u16,
        cx = layout.pitch() as u16,
        dx = layout.virtual_height() as u16
    },
    Err(ScanlineError::TooWide) => { ax = 0x024f },
    Err(ScanlineError::Failed) => { ax = 0x014f },
})]
pub(super) fn scanline(io: &IoBus, operation: u8, width: u16) -> Result<SvgaLayout, ScanlineError> {
    let layout = active(io).ok_or(ScanlineError::Failed)?;
    match operation {
        // In eight-bit modes the byte stride and pixel width are identical.
        0 | 2 => {
            let pitch = u32::from(width).max(layout.size()[0]);
            if pitch > layout.mode().max_pitch() {
                return Err(ScanlineError::TooWide);
            }
            let next = layout.with_pitch(pitch).ok_or(ScanlineError::Failed)?;
            write_register(io, dispi::VIRTUAL_WIDTH, next.pitch() as u16);
            Ok(next)
        }
        1 => Ok(layout),
        // Report capability independently of the current viewport origin.
        3 => layout
            .mode()
            .layout()
            .with_pitch(layout.mode().max_pitch())
            .ok_or(ScanlineError::Failed),
        _ => Err(ScanlineError::Failed),
    }
}

//! Logical framebuffer services use the device's checked layout through DISPI.
//! VBE 2.0 sections 4.9–4.10 define the extended selectors and error results:
//! https://www.phatcode.net/res/221/files/vbe20.pdf
use super::{available, read_register, write_register, Mode};
use relocade_machine::{real_mode_interrupt::interrupt, Clock, IoBus};
use relocade_vga::{registers::dispi, RetraceEvent, SvgaLayout, Vga};
use std::cell::RefCell;

#[cfg(test)]
mod tests;

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

#[interrupt(context(io), args(operation = bx, x = cx, y = dx), returns {
    Some([x, y]) => { ax = 0x004f, bh = 0, cx = x, dx = y },
    None => { ax = 0x014f },
})]
pub(super) async fn display_start<C: Clock + 'static>(
    io: &IoBus,
    vga: &RefCell<Vga<C>>,
    operation: u16,
    x: u16,
    y: u16,
) -> Option<[u16; 2]> {
    let layout = active(io)?;
    match operation {
        1 => Some(layout.origin().map(|value| value as u16)),
        0 | 0x80 => {
            layout.with_origin([u32::from(x), u32::from(y)])?;
            if operation == 0x80 {
                loop {
                    let wait = { vga.borrow_mut().wait_for_retrace()? };
                    let event = wait.await;
                    // Revalidation services the completed boundary before the origin changes.
                    if active(io) != Some(layout) {
                        return None;
                    }
                    if event == RetraceEvent::Started {
                        break;
                    }
                }
            }
            write_register(io, dispi::X_OFFSET, x);
            write_register(io, dispi::Y_OFFSET, y);
            Some([x, y])
        }
        _ => None, // BH is reserved; only the three complete BX values are valid.
    }
}

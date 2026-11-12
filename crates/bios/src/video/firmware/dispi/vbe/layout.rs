//! Logical framebuffer services use the card's checked packed layout.
//! VBE 2.0 sections 4.9–4.10 define the extended selectors and error results:
//! https://www.phatcode.net/res/221/files/vbe20.pdf
use super::available;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::{DispiAdapter, PackedLayout, RetraceEvent};
use std::cell::RefCell;

#[cfg(test)]
mod tests;

pub(super) fn active(card: &RefCell<DispiAdapter>) -> Option<PackedLayout> {
    if !available(card) {
        return None;
    }
    card.borrow().packed_layout()
}

pub(super) enum ScanlineError {
    Failed,
    TooWide,
}

#[interrupt(context(io), args(operation = bl, width = cx), returns {
    Ok(layout) => {
        ax = 0x004f,
        bx = layout.pitch() as u16,
        cx = layout.virtual_width() as u16,
        dx = layout.virtual_height() as u16
    },
    Err(ScanlineError::TooWide) => { ax = 0x024f },
    Err(ScanlineError::Failed) => { ax = 0x014f },
})]
pub(super) fn scanline(
    io: &IoBus,
    card: &RefCell<DispiAdapter>,
    operation: u8,
    width: u16,
) -> Result<PackedLayout, ScanlineError> {
    let layout = active(card).ok_or(ScanlineError::Failed)?;
    match operation {
        0 | 2 => {
            let pixel_bytes = layout.format().bytes_per_pixel();
            let pixels = if operation == 2 {
                u32::from(width).div_ceil(pixel_bytes)
            } else {
                u32::from(width)
            };
            let pitch = pixels.max(layout.size()[0]) * pixel_bytes;
            if pitch > layout.max_pitch() {
                return Err(ScanlineError::TooWide);
            }
            io.with_context(|context| card.borrow_mut().set_packed_pitch(context, pitch))
                .ok_or(ScanlineError::Failed)
        }
        1 => Ok(layout),
        // Report capability independently of the current viewport origin.
        3 => layout
            .with_origin([0, 0])
            .and_then(|layout| layout.with_pitch(layout.max_pitch()))
            .ok_or(ScanlineError::Failed),
        _ => Err(ScanlineError::Failed),
    }
}

#[interrupt(context(io), args(operation = bx, x = cx, y = dx), returns {
    Some([x, y]) => { ax = 0x004f, bh = 0, cx = x, dx = y },
    None => { ax = 0x014f },
})]
pub(super) async fn display_start(
    io: &IoBus,
    card: &RefCell<DispiAdapter>,
    operation: u16,
    x: u16,
    y: u16,
) -> Option<[u16; 2]> {
    let layout = active(card)?;
    match operation {
        1 => Some(layout.origin().map(|value| value as u16)),
        0 | 0x80 => {
            layout.with_origin([u32::from(x), u32::from(y)])?;
            if operation == 0x80 {
                loop {
                    let wait = { card.borrow_mut().wait_for_retrace()? };
                    let event = wait.await;
                    // Revalidation services the completed boundary before the origin changes.
                    if active(card) != Some(layout) {
                        return None;
                    }
                    if event == RetraceEvent::Started {
                        break;
                    }
                }
            }
            io.with_context(|context| card.borrow_mut().set_display_origin(context, [x, y]))
                .then_some([x, y])
        }
        _ => None, // BH is reserved; only the three complete BX values are valid.
    }
}

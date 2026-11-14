//! Logical framebuffer services use the active card's checked memory layout.
//! VBE 2.0 sections 4.9–4.10 define the extended selectors and error results:
//! https://www.phatcode.net/res/221/files/vbe20.pdf
use super::super::VideoFirmware;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::{RetraceEvent, Viewport};

#[cfg(test)]
mod tests;

pub(in crate::video::firmware) enum ScanlineError {
    Failed,
    TooWide,
}

pub(in crate::video::firmware) struct Scanline {
    bytes: u16,
    pixels: u16,
    lines: u16,
}

impl From<Viewport> for Scanline {
    fn from(layout: Viewport) -> Self {
        Self {
            bytes: layout.pitch() as u16,
            pixels: layout.virtual_width() as u16,
            lines: layout.virtual_height().min(u32::from(u16::MAX)) as u16,
        }
    }
}

#[interrupt(context(io), args(operation = bl, width = cx), returns {
    Ok(layout) => {
        ax = 0x004f,
        bx = layout.bytes,
        cx = layout.pixels,
        dx = layout.lines
    },
    Err(ScanlineError::TooWide) => { ax = 0x024f },
    Err(ScanlineError::Failed) => { ax = 0x014f },
})]
pub(in crate::video::firmware) fn scanline(
    io: &IoBus,
    firmware: &VideoFirmware,
    operation: u8,
    width: u16,
) -> Result<Scanline, ScanlineError> {
    let layout = firmware.viewport().ok_or(ScanlineError::Failed)?;
    match operation {
        0 | 2 => {
            let pitch = if operation == 2 {
                u32::from(width)
            } else {
                layout.pitch_for_width(u32::from(width))
            };
            if pitch > layout.max_pitch() {
                return Err(ScanlineError::TooWide);
            }
            io.with_context(|context| firmware.set_display_pitch(context, pitch))
                .map(Into::into)
                .ok_or(ScanlineError::Failed)
        }
        1 => Ok(layout.into()),
        3 => layout
            .maximum()
            .map(Into::into)
            .ok_or(ScanlineError::Failed),
        _ => Err(ScanlineError::Failed),
    }
}

#[interrupt(context(io), args(operation = bx, x = cx, y = dx), returns {
    Some([x, y]) => { ax = 0x004f, bh = 0, cx = x, dx = y },
    None => { ax = 0x014f },
})]
pub(in crate::video::firmware) async fn display_start(
    io: &IoBus,
    firmware: &VideoFirmware,
    operation: u16,
    x: u16,
    y: u16,
) -> Option<[u16; 2]> {
    firmware.packed_layout()?;
    let layout = firmware.viewport()?;
    match operation {
        1 => Some(layout.origin().map(|value| value as u16)),
        0 | 0x80 => {
            layout.with_origin([u32::from(x), u32::from(y)])?;
            if operation == 0x80 {
                loop {
                    let event = firmware.wait_for_retrace().await?;
                    // Revalidation services the completed boundary before the origin changes.
                    if firmware.viewport() != Some(layout) {
                        return None;
                    }
                    if event == RetraceEvent::Started {
                        break;
                    }
                }
            }
            io.with_context(|context| firmware.set_display_origin(context, [x, y]))
                .then_some([x, y])
        }
        _ => None, // BH is reserved; only the three complete BX values are valid.
    }
}

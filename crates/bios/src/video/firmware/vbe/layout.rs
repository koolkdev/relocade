//! Logical framebuffer services use the card's checked packed layout.
//! VBE 2.0 sections 4.9–4.10 define the extended selectors and error results:
//! https://www.phatcode.net/res/221/files/vbe20.pdf
use super::super::VideoFirmware;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::{PackedLayout, RetraceEvent};

#[cfg(test)]
mod tests;

pub(in crate::video::firmware) enum ScanlineError {
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
pub(in crate::video::firmware) fn scanline(
    io: &IoBus,
    firmware: &VideoFirmware,
    operation: u8,
    width: u16,
) -> Result<PackedLayout, ScanlineError> {
    let layout = firmware.packed_layout().ok_or(ScanlineError::Failed)?;
    match operation {
        0 | 2 => {
            let pixel_bytes = layout.format().bytes_per_pixel();
            let pixels = if operation == 2 {
                u32::from(width).div_ceil(pixel_bytes)
            } else {
                u32::from(width)
            };
            let pitch = pixels.max(layout.size()[0]) * pixel_bytes;
            if pitch > firmware.max_packed_pitch().ok_or(ScanlineError::Failed)? {
                return Err(ScanlineError::TooWide);
            }
            io.with_context(|context| firmware.set_packed_pitch(context, pitch))
                .ok_or(ScanlineError::Failed)
        }
        1 => Ok(layout),
        // Report capability independently of the current viewport origin.
        3 => firmware
            .max_packed_pitch()
            .and_then(|pitch| firmware.bounded_layout(pitch))
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
    let layout = firmware.packed_layout()?;
    match operation {
        1 => Some(layout.origin().map(|value| value as u16)),
        0 | 0x80 => {
            firmware
                .bounded_layout(layout.pitch())?
                .with_origin([u32::from(x), u32::from(y)])?;
            if operation == 0x80 {
                loop {
                    let event = firmware.wait_for_retrace().await?;
                    // Revalidation services the completed boundary before the origin changes.
                    if firmware.packed_layout() != Some(layout) {
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

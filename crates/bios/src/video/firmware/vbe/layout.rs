//! Logical framebuffer services use the active card's checked memory layout.
//! VBE 2.0 sections 4.9–4.10 define the extended selectors and error results:
//! https://www.phatcode.net/res/221/files/vbe20.pdf
use super::super::VideoFirmware;
use crate::video::{crtc_port, palette::write_raw_attribute, write_indexed};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus};
use relocade_vga::{registers::attribute, LegacyLayout, PackedLayout, RetraceEvent};

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

impl From<PackedLayout> for Scanline {
    fn from(layout: PackedLayout) -> Self {
        Self {
            bytes: layout.pitch() as u16,
            pixels: layout.virtual_width() as u16,
            lines: layout.virtual_height() as u16,
        }
    }
}

impl From<LegacyLayout> for Scanline {
    fn from(layout: LegacyLayout) -> Self {
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
    if let Some(layout) = firmware.legacy_layout() {
        return legacy_scanline(io, layout, operation, width).map(Into::into);
    }
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
                .map(Into::into)
                .ok_or(ScanlineError::Failed)
        }
        1 => Ok(layout.into()),
        // Report capability independently of the current viewport origin.
        3 => firmware
            .max_packed_pitch()
            .and_then(|pitch| firmware.bounded_layout(pitch))
            .map(Into::into)
            .ok_or(ScanlineError::Failed),
        _ => Err(ScanlineError::Failed),
    }
}

fn legacy_scanline(
    io: &IoBus,
    layout: LegacyLayout,
    operation: u8,
    width: u16,
) -> Result<LegacyLayout, ScanlineError> {
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
            let next = layout.with_pitch(pitch).ok_or(ScanlineError::Failed)?;
            program_legacy_layout(io, next);
            Ok(next)
        }
        1 => Ok(layout),
        3 => layout.maximum().ok_or(ScanlineError::Failed),
        _ => Err(ScanlineError::Failed),
    }
}

fn program_legacy_layout(io: &IoBus, layout: LegacyLayout) {
    let port = crtc_port(io);
    for (register, value) in layout.crtc_registers() {
        write_indexed(io, port, register, value);
    }
    write_raw_attribute(
        io,
        attribute::HORIZONTAL_PANNING,
        layout.horizontal_panning(),
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Viewport {
    Packed(PackedLayout),
    Legacy(LegacyLayout),
}

impl Viewport {
    fn current(firmware: &VideoFirmware) -> Option<Self> {
        firmware
            .packed_layout()
            .map(Self::Packed)
            .or_else(|| firmware.legacy_layout().map(Self::Legacy))
    }

    fn origin(self) -> [u32; 2] {
        match self {
            Self::Packed(layout) => layout.origin(),
            Self::Legacy(layout) => layout.origin(),
        }
    }

    fn with_origin(self, firmware: &VideoFirmware, origin: [u32; 2]) -> Option<Self> {
        match self {
            Self::Packed(layout) => firmware
                .bounded_layout(layout.pitch())?
                .with_origin(origin)
                .map(Self::Packed),
            Self::Legacy(layout) => layout.with_origin(origin).map(Self::Legacy),
        }
    }

    fn program(self, io: &IoBus, firmware: &VideoFirmware) -> bool {
        match self {
            Self::Packed(layout) => io.with_context(|context| {
                firmware.set_display_origin(context, layout.origin().map(|value| value as u16))
            }),
            Self::Legacy(layout) => {
                program_legacy_layout(io, layout);
                true
            }
        }
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
    let layout = Viewport::current(firmware)?;
    match operation {
        1 => Some(layout.origin().map(|value| value as u16)),
        0 | 0x80 => {
            let next = layout.with_origin(firmware, [u32::from(x), u32::from(y)])?;
            if operation == 0x80 {
                loop {
                    let event = firmware.wait_for_retrace().await?;
                    // Revalidation services the completed boundary before the origin changes.
                    if Viewport::current(firmware) != Some(layout) {
                        return None;
                    }
                    if event == RetraceEvent::Started {
                        break;
                    }
                }
            }
            next.program(io, firmware).then_some([x, y])
        }
        _ => None, // BH is reserved; only the three complete BX values are valid.
    }
}

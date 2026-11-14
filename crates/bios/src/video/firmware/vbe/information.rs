//! VBE 1.2 guest buffers shared by the selected card's firmware.
//! https://mircosoft.mzf.cz/texty/VESA12sp.htm (functions 00h and 01h).
use super::{
    super::{dispi, et4000, VideoFirmware},
    WINDOW_FUNCTION,
};
use crate::BIOS_SEGMENT;
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};
use relocade_vga::{PackedLayout, PixelFormat};

// Follows the legacy video parameter table (F000:A700–AE3F).
const OEM: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xb000,
};
const MODES: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xb020,
};

pub(in crate::video::firmware) fn initialize(
    memory: &mut MemoryView<'_>,
    oem: &[u8],
    modes: impl IntoIterator<Item = u16>,
) {
    assert!(oem.len() <= 32 && oem.last() == Some(&0));
    memory
        .write_backing(OEM.address(), oem)
        .expect("VBE OEM string");
    let bytes: Vec<_> = modes
        .into_iter()
        .chain([0xffff])
        .flat_map(u16::to_le_bytes)
        .collect();
    assert!(bytes.len() <= usize::from(WINDOW_FUNCTION.offset - MODES.offset));
    memory
        .write_backing(MODES.address(), &bytes)
        .expect("VBE mode list");
}

#[interrupt(context(memory), args(destination = ptr(es, di)),
    returns { Some(()) => { ax = 0x004f }, None => { ax = 0x014f } },
)]
pub(super) fn controller(
    memory: &mut MemoryView<'_>,
    firmware: &VideoFirmware,
    destination: FarPointer,
) -> Option<()> {
    firmware.ports_enabled().then_some(())?;
    let memory_bytes = firmware.video_memory_bytes()?;
    // Always the bounded 1.x structure, even if the caller requests "VBE2".
    let mut bytes = [0; 256];
    bytes[..4].copy_from_slice(b"VESA");
    bytes[4..6].copy_from_slice(&0x0102u16.to_le_bytes());
    bytes[6..10].copy_from_slice(&OEM.packed().to_le_bytes());
    // Capabilities = 0: VGA compatible, fixed six-bit DAC.
    bytes[14..18].copy_from_slice(&MODES.packed().to_le_bytes());
    bytes[18..20].copy_from_slice(&((memory_bytes / 65536) as u16).to_le_bytes());
    memory
        .write_wrapping(destination, &bytes)
        .expect("VBE controller information buffer");
    Some(())
}

pub(in crate::video::firmware) struct ModeInformation {
    pub layout: PackedLayout,
    pub windows: [WindowAccess; 2],
}

#[interrupt(context(memory), args(number = cx, destination = ptr(es, di)),
    returns { Some(()) => { ax = 0x004f }, None => { ax = 0x014f } },
)]
pub(super) fn mode(
    memory: &mut MemoryView<'_>,
    firmware: &VideoFirmware,
    number: u16,
    destination: FarPointer,
) -> Option<()> {
    let information = match firmware {
        VideoFirmware::Vga => return None,
        VideoFirmware::Dispi(card) => dispi::vbe::information::mode(card, number)?,
        VideoFirmware::Et4000Ax(card) => et4000::vbe::mode(card, number)?,
    };
    banked_mode(memory, destination, information.layout, information.windows);
    Some(())
}

#[derive(Clone, Copy)]
pub(in crate::video::firmware) enum WindowAccess {
    Absent,
    Read,
    Write,
    ReadWrite,
}

impl WindowAccess {
    fn attributes(self) -> u8 {
        match self {
            Self::Absent => 0,
            Self::Read => 3,
            Self::Write => 5,
            Self::ReadWrite => 7,
        }
    }
}

/// Describe 64-KiB banks at A000h, with the card's independent window permissions.
fn banked_mode(
    memory: &mut MemoryView<'_>,
    destination: FarPointer,
    layout: PackedLayout,
    windows: [WindowAccess; 2],
) {
    let [width, height] = layout.size().map(|value| value as u16);
    let mut bytes = [0; 256];
    // Supported color graphics, extended information, no BIOS text/pixel output.
    bytes[..2].copy_from_slice(&0x001bu16.to_le_bytes());
    for (index, window) in windows.into_iter().enumerate() {
        bytes[2 + index] = window.attributes();
        if !matches!(window, WindowAccess::Absent) {
            bytes[8 + index * 2..10 + index * 2].copy_from_slice(&0xa000u16.to_le_bytes());
        }
    }
    bytes[4..6].copy_from_slice(&64u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&64u16.to_le_bytes());
    bytes[12..16].copy_from_slice(&WINDOW_FUNCTION.packed().to_le_bytes());
    bytes[16..18].copy_from_slice(&(layout.pitch() as u16).to_le_bytes());
    bytes[18..20].copy_from_slice(&width.to_le_bytes());
    bytes[20..22].copy_from_slice(&height.to_le_bytes());
    bytes[22..28].copy_from_slice(&[8, 16, 1, layout.format().bits_per_pixel(), 1, 4]);
    let masks = match layout.format() {
        PixelFormat::Indexed8 => None,
        PixelFormat::Rgb555 => Some([5, 10, 5, 5, 5, 0, 1, 15, 0]),
        PixelFormat::Rgb565 => Some([5, 11, 6, 5, 5, 0, 0, 0, 0]),
        PixelFormat::Bgrx8888 => Some([8, 16, 8, 8, 8, 0, 8, 24, 2]),
    };
    if let Some(masks) = masks {
        bytes[27] = 6; // Direct color with a fixed ramp.
        bytes[31..40].copy_from_slice(&masks);
    }
    // No interleaved scanline banks; VBE counts additional complete display pages.
    bytes[29] = layout.image_pages().saturating_sub(1) as u8;
    bytes[30] = 1; // VBE 1.2's reserved page-function byte.
    memory
        .write_wrapping(destination, &bytes)
        .expect("VBE mode information buffer");
}

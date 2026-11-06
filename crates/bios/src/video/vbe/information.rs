//! Guest byte layouts from the VESA Super VGA BIOS Extension standard, version 1.2.
//! https://mircosoft.mzf.cz/texty/VESA12sp.htm (functions 00h and 01h).
use super::{available, Mode};
use crate::BIOS_SEGMENT;
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, IoBus, MemoryView};
use relocade_vga::{registers::dispi, VIDEO_MEMORY_BYTES};

// Follows the legacy video parameter table (F000:A700–AE3F).
const OEM: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xb000,
};
const MODES: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xb020,
};

pub(in super::super) fn initialize(memory: &mut MemoryView<'_>) {
    memory
        .write_backing(OEM.address(), b"Relocade banked SVGA\0")
        .expect("VBE OEM string");
    let mut modes = [0xff; 4];
    modes[..2].copy_from_slice(&Mode::INDEXED_640_480.number.to_le_bytes());
    memory
        .write_backing(MODES.address(), &modes)
        .expect("VBE mode list");
}

#[interrupt(context(memory, io), args(destination = ptr(es, di)),
    returns { true => { ax = 0x004f }, false => { ax = 0x014f } },
)]
pub(super) fn controller(memory: &mut MemoryView<'_>, io: &IoBus, destination: FarPointer) -> bool {
    if !available(io) {
        return false;
    }
    // This is the 256-byte 1.x structure, even if the caller requests "VBE2".
    let mut bytes = [0; 256];
    bytes[..4].copy_from_slice(b"VESA");
    bytes[4..6].copy_from_slice(&0x0102u16.to_le_bytes());
    bytes[6..10].copy_from_slice(&OEM.packed().to_le_bytes());
    // Capabilities = 0: VGA compatible, fixed six-bit DAC.
    bytes[14..18].copy_from_slice(&MODES.packed().to_le_bytes());
    bytes[18..20].copy_from_slice(&((VIDEO_MEMORY_BYTES / 65536) as u16).to_le_bytes());
    memory
        .write_wrapping(destination, &bytes)
        .expect("VBE controller information buffer");
    true
}

#[interrupt(context(memory, io), args(number = cx, destination = ptr(es, di)),
    returns { true => { ax = 0x004f }, false => { ax = 0x014f } },
)]
pub(super) fn mode(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    number: u16,
    destination: FarPointer,
) -> bool {
    let Some(mode) = Mode::from_number(number).filter(|_| available(io)) else {
        return false;
    };
    let mut bytes = [0; 256];
    // Supported color graphics, extended information, no BIOS text/pixel output.
    bytes[..2].copy_from_slice(&0x001bu16.to_le_bytes());
    bytes[2] = 7; // Window A exists and is readable and writable; no window B.
    let window_kib = (dispi::BANK_BYTES / 1024) as u16;
    bytes[4..6].copy_from_slice(&window_kib.to_le_bytes());
    bytes[6..8].copy_from_slice(&window_kib.to_le_bytes());
    bytes[8..10].copy_from_slice(&((dispi::BANK_START >> 4) as u16).to_le_bytes());
    // Null WinFuncPtr directs callers to INT 10h/4F05h.
    bytes[16..18].copy_from_slice(&mode.width.to_le_bytes()); // Eight-bit pixel pitch.
    bytes[18..20].copy_from_slice(&mode.width.to_le_bytes());
    bytes[20..22].copy_from_slice(&mode.height.to_le_bytes());
    bytes[22..28].copy_from_slice(&[8, 16, 1, mode.bits_per_pixel, 1, 4]);
    // No interleaved scanline banks or additional display pages.
    bytes[30] = 1; // VBE 1.2's reserved page-function byte.
    memory
        .write_wrapping(destination, &bytes)
        .expect("VBE mode information buffer");
    true
}

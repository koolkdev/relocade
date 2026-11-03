//! Guests install palette profiles through the BIOS save-pointer tables.
//! Mode setting applies their color and underline settings after guest font tables.
use super::{attributes, dac, PaletteLoad};
use crate::video::{crtc_port, tables::Pointer, write_indexed, CHARACTER_HEIGHT, MODE};
use relocade_machine::{FarPointer, IoBus, MemoryView};
use relocade_vga::registers::crtc;

pub(in crate::video) fn apply(memory: &mut MemoryView<'_>, io: &IoBus) {
    let mode = memory.read_u8(MODE).expect("video mode");
    let Some(header) = Pointer::PaletteProfile.get_override_header::<20>(memory, mode) else {
        return;
    };
    if PaletteLoad::for_mode_set(memory).loads_colors() {
        let count = u16::from_le_bytes([header[4], header[5]]);
        let first = u16::from_le_bytes([header[6], header[7]]);
        let source = FarPointer::from_packed(u32::from_le_bytes(header[8..12].try_into().unwrap()));
        attributes::load_range(memory, io, first, count, source);

        let count = u16::from_le_bytes([header[12], header[13]]);
        let first = u16::from_le_bytes([header[14], header[15]]);
        let source =
            FarPointer::from_packed(u32::from_le_bytes(header[16..20].try_into().unwrap()));
        if count <= 256 && first < 256 {
            // The shared DAC path applies grayscale summing and wraps the index.
            dac::load_palette(memory, io, first as u8, count, source);
        }
    }
    // Underlining remains independent of BL=31h palette-loading suppression.
    let underline = match header[0] as i8 {
        0 => return,
        1.. => memory
            .read_u8(CHARACTER_HEIGHT)
            .expect("font height")
            .wrapping_sub(1),
        _ => 31,
    };
    write_indexed(io, crtc_port(io), crtc::UNDERLINE_LOCATION, underline);
}

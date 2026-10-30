//! Guest-readable firmware fonts and their real-mode pointers.
use crate::BIOS_SEGMENT;
use relocade_machine::MemoryView;
use relocade_vga::registers::text_mode;

pub(super) struct Font {
    pub height: u8,
    pub bytes: &'static [u8],
    offset: u16,
}

const FONT_8: Font = Font {
    height: 8,
    bytes: text_mode::FONT,
    offset: 0x8000,
};
const FONT_14: Font = Font {
    height: 14,
    bytes: include_bytes!("../../../assets/modern-dos-8x14.bin"),
    offset: 0x8800,
};
const FONT_16: Font = Font {
    height: 16,
    bytes: include_bytes!("../../../assets/modern-dos-8x16.bin"),
    offset: 0x9600,
};
// Modern DOS needs no replacement glyphs for a ninth column. These tables
// contain the terminating character zero, not a second 256-character font.
const REPLACEMENTS_14: u16 = 0xa600;
const REPLACEMENTS_16: u16 = 0xa601;

impl Font {
    pub fn pointer(&self) -> u32 {
        far_pointer(self.offset)
    }
}

pub(super) fn for_height(height: u8) -> &'static Font {
    match height {
        8 => &FONT_8,
        14 => &FONT_14,
        16 => &FONT_16,
        _ => unreachable!("built-in font height"),
    }
}

pub(in crate::video) fn initialize(memory: &mut MemoryView<'_>) {
    let base = u32::from(BIOS_SEGMENT) << 4;
    for font in [&FONT_8, &FONT_14, &FONT_16] {
        memory
            .write_backing(base + u32::from(font.offset), font.bytes)
            .expect("BIOS font ROM");
    }
    memory
        .write_backing(base + u32::from(REPLACEMENTS_14), &[0, 0])
        .expect("BIOS font replacement tables");
    memory
        .write_u32(0x1f * 4, far_pointer(FONT_8.offset + 128 * 8))
        .expect("high character font vector");
}

pub(super) fn query(memory: &MemoryView<'_>, selector: u8) -> Option<u32> {
    Some(match selector {
        0 => memory
            .read_u32(0x1f * 4)
            .expect("high character font vector"),
        1 => memory.read_u32(0x43 * 4).expect("graphics font vector"),
        2 => FONT_14.pointer(),
        3 => FONT_8.pointer(),
        4 => far_pointer(FONT_8.offset + 128 * 8),
        5 => far_pointer(REPLACEMENTS_14),
        6 => FONT_16.pointer(),
        7 => far_pointer(REPLACEMENTS_16),
        _ => return None,
    })
}

fn far_pointer(offset: u16) -> u32 {
    (u32::from(BIOS_SEGMENT) << 16) | u32::from(offset)
}

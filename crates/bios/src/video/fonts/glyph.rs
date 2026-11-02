//! Read a graphics glyph through the guest's live BIOS font vectors.
use super::super::{modes, MODE};
use super::rom;
use relocade_machine::MemoryView;

pub(crate) struct Glyph {
    rows: [u8; 32],
    height: usize,
}

impl Glyph {
    pub fn read(memory: &MemoryView<'_>, character: u8, height: u16) -> Option<Self> {
        if !(1..=32).contains(&height) {
            return None;
        }
        // Only CGA-compatible modes split at character 80h. Planar and indexed
        // VGA modes use INT 43h for all 256 glyphs, including eight-high fonts.
        let mode = memory.read_u8(MODE).expect("video mode");
        let packed = matches!(mode, modes::GRAPHICS_320X200_4..=modes::GRAPHICS_640X200_2);
        let (selector, index) = if packed && character >= 128 {
            (0, character - 128)
        } else {
            (1, character)
        };
        let pointer = rom::query(memory, selector).expect("graphics font vector");
        let source = pointer.wrapping_add(usize::from(index) * usize::from(height));
        let mut glyph = Self {
            rows: [0; 32],
            height: usize::from(height),
        };
        memory
            .read_wrapping(source, &mut glyph.rows[..glyph.height])
            .expect("graphics font bytes");
        Some(glyph)
    }

    pub fn rows(&self) -> &[u8] {
        &self.rows[..self.height]
    }
}

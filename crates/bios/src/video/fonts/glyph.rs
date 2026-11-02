//! Read and recognize graphics glyphs through the guest's live BIOS font vectors.
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
        let (selector, index) = if uses_split_font(memory) && character >= 128 {
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

    /// IBM searches in character-code order, including blank and duplicate glyphs.
    pub fn recognize(memory: &MemoryView<'_>, rows: &[u8]) -> u8 {
        let last = if uses_split_font(memory)
            && rom::query(memory, 0)
                .expect("high character font vector")
                .packed()
                == 0
        {
            127 // A null INT 1Fh vector disables the upper-half search.
        } else {
            255
        };
        (0..=last)
            .find(|&character| {
                Self::read(memory, character, rows.len() as u16)
                    .is_some_and(|glyph| glyph.rows() == rows)
            })
            .unwrap_or(0)
    }
}

fn uses_split_font(memory: &MemoryView<'_>) -> bool {
    matches!(
        memory.read_u8(MODE).expect("video mode"),
        modes::GRAPHICS_320X200_4..=modes::GRAPHICS_640X200_2
    )
}

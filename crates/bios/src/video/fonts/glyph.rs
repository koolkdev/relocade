//! Read a graphics glyph through the guest's live BIOS font vectors.
use super::super::buffer_addresses;
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
        // Eight-high fonts split at character 80h: INT 1Fh supplies the upper
        // half. Taller fonts use INT 43h for all 256 glyphs.
        let (selector, index) = if height == 8 && character >= 128 {
            (0, character - 128)
        } else {
            (1, character)
        };
        let pointer = rom::query(memory, selector).expect("graphics font vector");
        let offset = (pointer as u16).wrapping_add(u16::from(index) * height);
        let mut glyph = Self {
            rows: [0; 32],
            height: usize::from(height),
        };
        for (row, address) in glyph.rows.iter_mut().zip(buffer_addresses(
            (pointer >> 16) << 4,
            offset,
            u32::from(height),
        )) {
            *row = memory.read_u8(address).expect("graphics font byte");
        }
        Some(glyph)
    }

    pub fn rows(&self) -> &[u8] {
        &self.rows[..self.height]
    }
}

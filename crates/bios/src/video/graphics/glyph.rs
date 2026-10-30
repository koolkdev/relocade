//! Glyph rows become masked planar writes or eight indexed pixels through MMIO.
use super::{access::PlanarAccess, Page};
use relocade_machine::{IoBus, MemoryView};

impl Page {
    /// Positions are bounded (row, column) cells in the BIOS character grid.
    /// The caller reads the glyph before VGA access changes, even for a font in VRAM.
    pub fn write_glyphs(
        &self,
        memory: &mut MemoryView<'_>,
        io: &IoBus,
        positions: impl Iterator<Item = (u16, u16)>,
        glyph: &[u8],
        color: u8,
    ) {
        let access = self.mode.planar.then(|| PlanarAccess::new(io));
        for (row, column) in positions {
            for (scanline, &bits) in glyph.iter().enumerate() {
                let address = self.address(column * 8, row * glyph.len() as u16 + scanline as u16);
                if let Some(access) = &access {
                    // Replacement draws black behind the glyph; XOR leaves its
                    // zero bits alone. Both operations preserve adjacent cells.
                    if color & 0x80 == 0 {
                        access.write_masked(memory, address, !bits, 0);
                    }
                    access.write_masked(memory, address, bits, color);
                } else {
                    let pixels: [u8; 8] =
                        std::array::from_fn(|x| if bits & (0x80 >> x) != 0 { color } else { 0 });
                    memory.write(address, &pixels).expect("graphics glyph row");
                }
            }
        }
    }
}

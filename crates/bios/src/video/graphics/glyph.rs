//! Glyph rows become planar masks, indexed pixels or packed bytes through MMIO.
use super::{access::GraphicsAccess, Layout, Page};
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
        let layout = self.mode.layout;
        let access = GraphicsAccess::new(io, layout.write_planes());
        for (row, column) in positions {
            for (scanline, &bits) in glyph.iter().enumerate() {
                let address = self.address(column * 8, row * glyph.len() as u16 + scanline as u16);
                if layout == Layout::Planar {
                    // Replacement draws black behind the glyph; XOR leaves its
                    // zero bits alone. Both operations preserve adjacent cells.
                    if color & 0x80 == 0 {
                        access.write_planar_masked(memory, address, !bits, 0);
                    }
                    access.write_planar_masked(memory, address, bits, color);
                } else {
                    let mut bytes = [0; 8];
                    let pixels_per_byte = layout.pixels_per_byte();
                    // IBM VGA mode 06h draws the foreground bit regardless of
                    // BL's low bits; pixel writes still use AL's requested bit.
                    let foreground = if layout == Layout::Packed1 {
                        1
                    } else {
                        color & layout.pixel_mask()
                    };
                    for x in 0..8 {
                        if bits & (0x80 >> x) != 0 {
                            bytes[usize::from(x / pixels_per_byte)] |=
                                foreground << layout.shift(x);
                        }
                    }
                    let bytes = &mut bytes[..usize::from(8 / pixels_per_byte)];
                    if layout != Layout::Indexed256 && color & 0x80 != 0 {
                        for (offset, byte) in bytes.iter_mut().enumerate() {
                            *byte ^= memory
                                .read_u8(address + offset as u32)
                                .expect("glyph XOR byte");
                        }
                    }
                    memory.write(address, bytes).expect("graphics glyph row");
                }
            }
        }
    }
}

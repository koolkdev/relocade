use crate::{registers::Registers, TextCell};

const PLANE_BYTES: usize = 65536;

/// Four independent VGA planes. CPU reads latch all planes; scanout reads do not
/// disturb those latches. Plane addressing wraps at the hardware's 16-bit limit.
pub(super) struct VideoMemory {
    planes: [Box<[u8]>; 4],
    latches: [u8; 4],
}

impl Default for VideoMemory {
    fn default() -> Self {
        let mut memory = Self {
            planes: std::array::from_fn(|_| vec![0; PLANE_BYTES].into_boxed_slice()),
            latches: [0; 4],
        };
        // Match the bootstrap text registers. Guests can replace these bytes
        // through normal plane-two writes; scanout has no private font copy.
        for (character, glyph) in crate::registers::text_mode::FONT
            .as_chunks::<8>()
            .0
            .iter()
            .enumerate()
        {
            memory.planes[2][character * 32..character * 32 + 8].copy_from_slice(glyph);
        }
        memory
    }
}

impl VideoMemory {
    fn index(offset: u32, registers: &Registers) -> usize {
        let address_bits = if registers.sequencer.extended_memory() {
            16
        } else {
            14
        };
        let offset = if registers.sequencer.chain_four() {
            offset & !3
        } else if registers.graphics.chain_odd_even() {
            // Only the 128-KiB aperture exposes A16. The IBM VGA target has
            // no programmable page-select input for the smaller apertures.
            (offset & !1) | ((offset >> 16) & 1)
        } else {
            offset
        };
        (offset & ((1 << address_bits) - 1)) as usize
    }

    pub fn read(&mut self, address: u32, registers: &Registers) -> u8 {
        let graphics = &registers.graphics;
        let Some(offset) = graphics.memory_offset(address) else {
            return 0xff;
        };
        let index = Self::index(offset, registers);
        self.latches = std::array::from_fn(|plane| self.planes[plane][index]);
        if graphics.compare_reads() {
            return graphics.compare(self.latches);
        }
        let plane = if registers.sequencer.chain_four() {
            (offset & 3) as usize
        } else if graphics.odd_even_reads() {
            (graphics.read_plane() & 2) | (offset & 1) as usize
        } else {
            graphics.read_plane()
        };
        self.latches[plane]
    }

    pub fn write(&mut self, address: u32, value: u8, registers: &Registers) {
        let Some(offset) = registers.graphics.memory_offset(address) else {
            return;
        };
        let index = Self::index(offset, registers);
        let planes = registers.sequencer.write_planes(offset);
        let values = registers.graphics.write_data(value, self.latches);
        for (plane, value) in values.into_iter().enumerate() {
            if planes & (1 << plane) != 0 {
                self.planes[plane][index] = value;
            }
        }
    }

    pub fn cell(&self, address: u16) -> TextCell {
        TextCell {
            character: self.planes[0][usize::from(address)],
            attribute: self.planes[1][usize::from(address)],
        }
    }

    pub fn plane(&self, plane: usize, address: u16) -> u8 {
        self.planes[plane][usize::from(address)]
    }
}

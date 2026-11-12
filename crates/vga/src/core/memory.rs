//! VGA CPU access semantics over memory supplied by the complete card.
use super::VgaCore;
use crate::{memory::VideoMemory, Registers};

impl VgaCore {
    pub(crate) fn read_memory(&mut self, memory: &VideoMemory, address: u32, bytes: u8) -> u64 {
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.read_vga_byte(memory, address + index as u32);
        }
        u64::from_le_bytes(value)
    }

    pub(crate) fn write_memory(
        &mut self,
        memory: &mut VideoMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            self.write_vga_byte(memory, address + index as u32, *byte);
        }
    }

    fn index(offset: u32, registers: &Registers) -> usize {
        let address_bits = if registers.sequencer.extended_memory() {
            16
        } else {
            14
        };
        let offset = if registers.sequencer.chain_four() {
            // Chain-4 uses CPU A0/A1 for plane selection. The plane address
            // receives A14/A15 on those pins, matching CRTC doubleword output.
            (offset & !3) | ((offset >> 14) & 3)
        } else if registers.graphics.chain_odd_even() {
            // Only the 128-KiB aperture exposes A16. The IBM VGA target has
            // no programmable page-select input for the smaller apertures.
            (offset & !1) | ((offset >> 16) & 1)
        } else {
            offset
        };
        (offset & ((1 << address_bits) - 1)) as usize
    }

    fn read_vga_byte(&mut self, memory: &VideoMemory, address: u32) -> u8 {
        let registers = &self.registers;
        let graphics = &registers.graphics;
        let Some(offset) = graphics.memory_offset(address) else {
            return 0xff;
        };
        let index = Self::index(offset, registers);
        self.latches =
            std::array::from_fn(|plane| memory.read_physical((index * 4 + plane) as u32));
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

    fn write_vga_byte(&mut self, memory: &mut VideoMemory, address: u32, value: u8) {
        let registers = &self.registers;
        let Some(offset) = registers.graphics.memory_offset(address) else {
            return;
        };
        let index = Self::index(offset, registers);
        let planes = registers.sequencer.write_planes(offset);
        let values = registers.graphics.write_data(value, self.latches);
        for (plane, value) in values.into_iter().enumerate() {
            if planes & (1 << plane) != 0 {
                memory.write_physical((index * 4 + plane) as u32, value);
            }
        }
    }
}

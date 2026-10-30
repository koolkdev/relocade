//! A graphics operation owns temporary CPU access registers until completion.
use super::super::{read_indexed, write_indexed};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::{graphics, sequencer, GRAPHICS_INDEX_PORT, SEQUENCER_INDEX_PORT};

pub(super) struct GraphicsAccess<'a> {
    io: &'a IoBus,
    map_mask: u8,
    registers: [(u8, u8); 5],
    mode: u8,
    seq_index: u32,
    gc_index: u32,
}

impl<'a> GraphicsAccess<'a> {
    pub fn new(io: &'a IoBus, planes: u8) -> Self {
        let seq_index = io.read_port(SEQUENCER_INDEX_PORT, 1);
        let gc_index = io.read_port(GRAPHICS_INDEX_PORT, 1);
        let map_mask = read_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK);
        let registers = [
            graphics::MODE,
            graphics::DATA_ROTATE,
            graphics::READ_MAP_SELECT,
            graphics::BIT_MASK,
            graphics::ENABLE_SET_RESET,
        ]
        .map(|index| (index, read_indexed(io, GRAPHICS_INDEX_PORT, index)));
        let mode = registers[0].1 & !0x0b; // Read mode zero; write mode supplied by each operation.
        write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK, planes);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::MODE, mode);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::DATA_ROTATE, 0);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::BIT_MASK, 255);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::ENABLE_SET_RESET, 0);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::READ_MAP_SELECT, 0);
        Self {
            io,
            map_mask,
            registers,
            mode,
            seq_index,
            gc_index,
        }
    }

    pub fn read_planar_pixel(&self, memory: &MemoryView<'_>, address: u32, mask: u8) -> u8 {
        let mut color = 0;
        for plane in 0..4 {
            write_indexed(
                self.io,
                GRAPHICS_INDEX_PORT,
                graphics::READ_MAP_SELECT,
                plane,
            );
            if memory.read_u8(address).expect("graphics pixel") & mask != 0 {
                color |= 1 << plane;
            }
        }
        color
    }

    pub fn write_planar_masked(
        &self,
        memory: &mut MemoryView<'_>,
        address: u32,
        mask: u8,
        color: u8,
    ) {
        write_indexed(self.io, GRAPHICS_INDEX_PORT, graphics::MODE, self.mode | 2);
        write_indexed(
            self.io,
            GRAPHICS_INDEX_PORT,
            graphics::DATA_ROTATE,
            if color & 0x80 != 0 { 0x18 } else { 0 },
        );
        write_indexed(self.io, GRAPHICS_INDEX_PORT, graphics::BIT_MASK, mask);
        memory.read_u8(address).expect("graphics write latches");
        memory
            .write_u8(address, color)
            .expect("masked graphics write");
    }

    pub fn copy_planar(
        &self,
        memory: &mut MemoryView<'_>,
        source: u32,
        destination: u32,
        bytes: usize,
    ) {
        write_indexed(self.io, GRAPHICS_INDEX_PORT, graphics::MODE, self.mode | 1);
        // Each read captures all four planes. Its matching write must happen
        // before another read replaces those latches; host row buffering cannot do this.
        for offset in 0..bytes as u32 {
            memory
                .read_u8(source + offset)
                .expect("graphics scroll latches");
            memory
                .write_u8(destination + offset, 0)
                .expect("graphics scroll destination");
        }
    }

    pub fn fill_planar(
        &self,
        memory: &mut MemoryView<'_>,
        destination: u32,
        bytes: usize,
        color: u8,
    ) {
        write_indexed(self.io, GRAPHICS_INDEX_PORT, graphics::MODE, self.mode | 2);
        write_indexed(self.io, GRAPHICS_INDEX_PORT, graphics::DATA_ROTATE, 0);
        write_indexed(self.io, GRAPHICS_INDEX_PORT, graphics::BIT_MASK, 255);
        // Full masks replace every pixel. Bit seven is never XOR for a scroll fill.
        for offset in 0..bytes as u32 {
            memory
                .write_u8(destination + offset, color)
                .expect("graphics scroll fill");
        }
    }
}

impl Drop for GraphicsAccess<'_> {
    fn drop(&mut self) {
        for (index, value) in self.registers {
            write_indexed(self.io, GRAPHICS_INDEX_PORT, index, value);
        }
        write_indexed(
            self.io,
            SEQUENCER_INDEX_PORT,
            sequencer::MAP_MASK,
            self.map_mask,
        );
        self.io.write_port(SEQUENCER_INDEX_PORT, 1, self.seq_index);
        self.io.write_port(GRAPHICS_INDEX_PORT, 1, self.gc_index);
    }
}

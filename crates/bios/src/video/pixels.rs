//! BIOS pixel access in the layouts established by mode setting. All writes
//! go through VGA MMIO; planar updates use its latches and write mode two.
use super::{modes, read_indexed, write_indexed, MODE};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::{graphics, sequencer, GRAPHICS_INDEX_PORT, SEQUENCER_INDEX_PORT};

pub(super) enum Pixel {
    Indexed { address: u32 },
    Planar { address: u32, mask: u8 },
}

impl Pixel {
    /// Single-page modes ignore BH. Mode 0Dh pages occupy 8 KiB per plane.
    /// Out-of-bounds calls follow the BIOS unsupported-service policy.
    pub(super) fn locate(memory: &MemoryView<'_>, page: u8, x: u16, y: u16) -> Option<Self> {
        let mode = memory.read_u8(MODE).expect("video mode");
        let (width, height, page_offset) = match mode {
            modes::GRAPHICS_320X200_256 if x < 320 && y < 200 => {
                return Some(Self::Indexed {
                    address: 0xa0000 + u32::from(y) * 320 + u32::from(x),
                });
            }
            modes::GRAPHICS_320X200_16 if page < 8 => (320, 200, u32::from(page) * 0x2000),
            modes::GRAPHICS_640X480_16 => (640, 480, 0),
            _ => return None,
        };
        (x < width && y < height).then_some(Self::Planar {
            address: 0xa0000 + page_offset + u32::from(y) * u32::from(width / 8) + u32::from(x / 8),
            mask: 0x80 >> (x % 8),
        })
    }

    pub(super) fn read(self, memory: &MemoryView<'_>, io: &IoBus) -> u8 {
        let (address, mask) = match self {
            Self::Indexed { address } => return memory.read_u8(address).expect("graphics pixel"),
            Self::Planar { address, mask } => (address, mask),
        };
        let mode = read_indexed(io, GRAPHICS_INDEX_PORT, graphics::MODE);
        let read_map = read_indexed(io, GRAPHICS_INDEX_PORT, graphics::READ_MAP_SELECT);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::MODE, mode & !8);
        let mut color = 0;
        for plane in 0..4 {
            write_indexed(io, GRAPHICS_INDEX_PORT, graphics::READ_MAP_SELECT, plane);
            if memory.read_u8(address).expect("graphics pixel") & mask != 0 {
                color |= 1 << plane;
            }
        }
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::READ_MAP_SELECT, read_map);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::MODE, mode);
        color
    }

    pub(super) fn write(self, memory: &mut MemoryView<'_>, io: &IoBus, color: u8) {
        let (address, mask) = match self {
            Self::Indexed { address } => {
                // In 256-color modes bit seven is part of the color, not XOR.
                memory.write_u8(address, color).expect("graphics pixel");
                return;
            }
            Self::Planar { address, mask } => (address, mask),
        };
        let map_mask = read_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK);
        let mode = read_indexed(io, GRAPHICS_INDEX_PORT, graphics::MODE);
        let rotate = read_indexed(io, GRAPHICS_INDEX_PORT, graphics::DATA_ROTATE);
        let bit_mask = read_indexed(io, GRAPHICS_INDEX_PORT, graphics::BIT_MASK);
        write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK, 15);
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::MODE, (mode & !3) | 2);
        write_indexed(
            io,
            GRAPHICS_INDEX_PORT,
            graphics::DATA_ROTATE,
            if color & 0x80 != 0 { 0x18 } else { 0 },
        );
        write_indexed(io, GRAPHICS_INDEX_PORT, graphics::BIT_MASK, mask);
        // One read loads every plane's latch. The bit mask then preserves all
        // neighboring pixels, while mode two expands the four color bits.
        memory.read_u8(address).expect("graphics pixel latches");
        memory.write_u8(address, color).expect("graphics pixel");
        for (index, value) in [
            (graphics::MODE, mode),
            (graphics::DATA_ROTATE, rotate),
            (graphics::BIT_MASK, bit_mask),
        ] {
            write_indexed(io, GRAPHICS_INDEX_PORT, index, value);
        }
        write_indexed(io, SEQUENCER_INDEX_PORT, sequencer::MAP_MASK, map_mask);
    }
}

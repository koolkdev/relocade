//! Graphics Controller registers, IBM VGA Function pages 2-78–2-88.

use super::text_mode;

pub const SET_RESET: u8 = 0x00;
pub const ENABLE_SET_RESET: u8 = 0x01;
pub const COLOR_COMPARE: u8 = 0x02;
pub const DATA_ROTATE: u8 = 0x03;
pub const READ_MAP_SELECT: u8 = 0x04;
pub const MODE: u8 = 0x05;
pub const MISCELLANEOUS: u8 = 0x06;
pub const COLOR_DONT_CARE: u8 = 0x07;
pub const BIT_MASK: u8 = 0x08;

pub(crate) struct Graphics {
    index: u8,
    pub(super) registers: [u8; 9],
}

impl Default for Graphics {
    fn default() -> Self {
        let mut registers = [0; 9];
        for &(index, value) in text_mode::GRAPHICS_REGISTERS {
            registers[usize::from(index)] = value;
        }
        Self {
            index: 0,
            registers,
        }
    }
}

impl Graphics {
    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn select(&mut self, index: u8) {
        self.index = index & 0x0f;
    }

    fn register(&self, index: u8) -> u8 {
        self.registers[usize::from(index)]
    }

    pub fn read(&self) -> u8 {
        if self.index <= BIT_MASK {
            self.register(self.index)
        } else {
            0xff
        }
    }

    pub fn write(&mut self, value: u8) {
        let mask = match self.index {
            SET_RESET | ENABLE_SET_RESET | COLOR_COMPARE | MISCELLANEOUS | COLOR_DONT_CARE => 0x0f,
            DATA_ROTATE => 0x1f,
            READ_MAP_SELECT => 0x03,
            MODE => 0x7b,
            BIT_MASK => 0xff,
            _ => return,
        };
        self.registers[usize::from(self.index)] = value & mask;
    }

    pub fn memory_offset(&self, address: u32) -> Option<u32> {
        let (start, size) = match (self.register(MISCELLANEOUS) >> 2) & 3 {
            0 => (0xa0000, 0x20000),
            1 => (0xa0000, 0x10000),
            2 => (0xb0000, 0x08000),
            _ => (0xb8000, 0x08000),
        };
        address.checked_sub(start).filter(|offset| *offset < size)
    }

    pub fn odd_even_reads(&self) -> bool {
        self.register(MODE) & 0x10 != 0
    }

    pub fn chain_odd_even(&self) -> bool {
        self.register(MISCELLANEOUS) & 2 != 0
    }

    pub fn read_plane(&self) -> usize {
        usize::from(self.register(READ_MAP_SELECT))
    }

    pub fn compare_reads(&self) -> bool {
        self.register(MODE) & 8 != 0
    }

    pub fn compare(&self, latches: [u8; 4]) -> u8 {
        let mut matches = 0xff;
        for (plane, latch) in latches.into_iter().enumerate() {
            if self.register(COLOR_DONT_CARE) & (1 << plane) != 0 {
                let color = if self.register(COLOR_COMPARE) & (1 << plane) != 0 {
                    0xff
                } else {
                    0
                };
                matches &= !(latch ^ color);
            }
        }
        matches
    }

    /// The CPU read latches supply both logical-operation input and masked bits.
    /// A write never reloads them from the destination address.
    pub fn write_data(&self, value: u8, latches: [u8; 4]) -> [u8; 4] {
        let mode = self.register(MODE) & 3;
        if mode == 1 {
            return latches;
        }
        let rotated = value.rotate_right(u32::from(self.register(DATA_ROTATE) & 7));
        let mask = self.register(BIT_MASK) & if mode == 3 { rotated } else { 0xff };
        std::array::from_fn(|plane| {
            let set_reset = if self.register(SET_RESET) & (1 << plane) != 0 {
                0xff
            } else {
                0
            };
            let source = match mode {
                0 if self.register(ENABLE_SET_RESET) & (1 << plane) != 0 => set_reset,
                0 => rotated,
                2 => {
                    if value & (1 << plane) != 0 {
                        0xff
                    } else {
                        0
                    }
                }
                _ => set_reset,
            };
            let latch = latches[plane];
            let combined = match (self.register(DATA_ROTATE) >> 3) & 3 {
                0 => source,
                1 => source & latch,
                2 => source | latch,
                _ => source ^ latch,
            };
            (combined & mask) | (latch & !mask)
        })
    }
}

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
    registers: [u8; 9],
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
}

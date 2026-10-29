//! CRTC register storage and field decoding. Indices follow IBM VGA Function
//! pages 2-55–2-76. This owner stores register state, not live raster counters.

use super::text_mode;

pub const HORIZONTAL_TOTAL: u8 = 0x00;
pub const HORIZONTAL_DISPLAY_END: u8 = 0x01;
pub const HORIZONTAL_BLANK_START: u8 = 0x02;
pub const HORIZONTAL_BLANK_END: u8 = 0x03;
pub const HORIZONTAL_RETRACE_START: u8 = 0x04;
pub const HORIZONTAL_RETRACE_END: u8 = 0x05;
pub const VERTICAL_TOTAL: u8 = 0x06;
pub const OVERFLOW: u8 = 0x07;
pub const PRESET_ROW_SCAN: u8 = 0x08;
pub const MAXIMUM_SCAN_LINE: u8 = 0x09;
pub const CURSOR_START: u8 = 0x0a;
pub const CURSOR_END: u8 = 0x0b;
pub const START_ADDRESS_HIGH: u8 = 0x0c;
pub const START_ADDRESS_LOW: u8 = 0x0d;
pub const CURSOR_ADDRESS_HIGH: u8 = 0x0e;
pub const CURSOR_ADDRESS_LOW: u8 = 0x0f;
pub const VERTICAL_RETRACE_START: u8 = 0x10;
pub const VERTICAL_RETRACE_END: u8 = 0x11;
pub const VERTICAL_DISPLAY_END: u8 = 0x12;
pub const OFFSET: u8 = 0x13;
pub const UNDERLINE_LOCATION: u8 = 0x14;
pub const VERTICAL_BLANK_START: u8 = 0x15;
pub const VERTICAL_BLANK_END: u8 = 0x16;
pub const MODE_CONTROL: u8 = 0x17;
pub const LINE_COMPARE: u8 = 0x18;

pub const PROTECT_TIMING_REGISTERS: u8 = 1 << 7;
const LINE_COMPARE_OVERFLOW: u8 = 1 << 4;

pub(crate) struct Crtc {
    index: u8,
    registers: [u8; 0x19],
}

impl Default for Crtc {
    fn default() -> Self {
        let mut registers = [0; 0x19];
        for &(index, value) in text_mode::CRTC_REGISTERS {
            registers[usize::from(index)] = value;
        }
        Self {
            index: 0,
            registers,
        }
    }
}

impl Crtc {
    pub fn register(&self, index: u8) -> u8 {
        self.registers[usize::from(index)]
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn select(&mut self, index: u8) {
        self.index = index & 0x1f;
    }

    pub fn read(&self) -> u8 {
        match self.index {
            HORIZONTAL_TOTAL..=LINE_COMPARE => self.register(self.index),
            _ => 0xff,
        }
    }

    pub fn write(&mut self, mut value: u8) {
        let index = self.index;
        if index <= OVERFLOW && self.register(VERTICAL_RETRACE_END) & PROTECT_TIMING_REGISTERS != 0
        {
            // CRTC 11 bit 7 protects 00–07 except the line-compare overflow bit.
            if index == OVERFLOW {
                self.registers[usize::from(OVERFLOW)] = (self.register(OVERFLOW)
                    & !LINE_COMPARE_OVERFLOW)
                    | (value & LINE_COMPARE_OVERFLOW);
            }
            return;
        }
        match index {
            CURSOR_START => value &= 0x3f, // Scanline and cursor-disable bits.
            CURSOR_END => value &= 0x7f,   // Scanline and cursor-skew bits.
            PRESET_ROW_SCAN | UNDERLINE_LOCATION => value &= 0x7f,
            HORIZONTAL_TOTAL..=LINE_COMPARE => {}
            _ => return,
        }
        self.registers[usize::from(index)] = value;
    }

    pub fn start_address(&self) -> u16 {
        u16::from_be_bytes([
            self.register(START_ADDRESS_HIGH),
            self.register(START_ADDRESS_LOW),
        ])
    }

    pub fn cursor_address(&self) -> u16 {
        u16::from_be_bytes([
            self.register(CURSOR_ADDRESS_HIGH),
            self.register(CURSOR_ADDRESS_LOW),
        ])
    }

    pub fn cursor_start(&self) -> u8 {
        self.register(CURSOR_START)
    }

    pub fn cursor_end(&self) -> u8 {
        self.register(CURSOR_END)
    }
}

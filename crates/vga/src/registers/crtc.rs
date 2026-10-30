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

/// CRTC values and the current CPU register index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crtc {
    index: u8,
    pub values: [u8; 0x19],
}

impl Default for Crtc {
    fn default() -> Self {
        let mut values = [0; 0x19];
        for &(index, value) in text_mode::CRTC_REGISTERS {
            values[usize::from(index)] = value;
        }
        Self { index: 0, values }
    }
}

impl Crtc {
    fn register(&self, index: u8) -> u8 {
        self.values[usize::from(index)]
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub(crate) fn select(&mut self, index: u8) {
        self.index = index & 0x1f;
    }

    pub(crate) fn read(&self) -> u8 {
        match self.index {
            HORIZONTAL_TOTAL..=LINE_COMPARE => self.register(self.index),
            _ => 0xff,
        }
    }

    pub(crate) fn write(&mut self, mut value: u8) {
        let index = self.index;
        if index <= OVERFLOW && self.register(VERTICAL_RETRACE_END) & PROTECT_TIMING_REGISTERS != 0
        {
            // CRTC 11 bit 7 protects 00–07 except the line-compare overflow bit.
            if index == OVERFLOW {
                self.values[usize::from(OVERFLOW)] = (self.register(OVERFLOW)
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
        self.values[usize::from(index)] = value;
    }

    pub(crate) fn start_address(&self) -> u16 {
        u16::from_be_bytes([
            self.register(START_ADDRESS_HIGH),
            self.register(START_ADDRESS_LOW),
        ])
    }

    pub(crate) fn cursor_address(&self) -> u16 {
        u16::from_be_bytes([
            self.register(CURSOR_ADDRESS_HIGH),
            self.register(CURSOR_ADDRESS_LOW),
        ])
    }

    pub(crate) fn cursor_start(&self) -> u8 {
        self.register(CURSOR_START)
    }

    pub(crate) fn cursor_end(&self) -> u8 {
        self.register(CURSOR_END)
    }

    /// Translate the CRTC address counter to physical plane address pins.
    /// Word/doubleword modes rotate high counter bits into the low output bits
    /// (IBM VGA p2-75); they do not compact CPU accesses in video memory.
    pub(crate) fn display_address(&self, counter: u16, row_scan: u8) -> u16 {
        let mode = self.register(MODE_CONTROL);
        let mut address = if self.register(UNDERLINE_LOCATION) & 0x40 != 0 {
            (counter << 2) | ((counter >> 12) & 3)
        } else if mode & 0x40 == 0 {
            let wrap_bit = if mode & 0x20 != 0 { 15 } else { 13 };
            (counter << 1) | ((counter >> wrap_bit) & 1)
        } else {
            counter
        };
        if mode & 1 == 0 {
            address = (address & !0x2000) | (u16::from(row_scan & 1) << 13);
        }
        if mode & 2 == 0 {
            address = (address & !0x4000) | (u16::from(row_scan & 2) << 13);
        }
        address
    }
}

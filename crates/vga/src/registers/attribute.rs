//! Attribute Controller register access, IBM VGA Function pages 2-89–2-96.

use super::text_mode;

pub const MODE_CONTROL: u8 = 0x10;
pub const OVERSCAN_COLOR: u8 = 0x11;
pub const COLOR_PLANE_ENABLE: u8 = 0x12;
pub const HORIZONTAL_PANNING: u8 = 0x13;
pub const COLOR_SELECT: u8 = 0x14;
pub const PALETTE_ADDRESS_SOURCE: u8 = 0x20;

pub(crate) struct Attribute {
    address: u8,
    data_next: bool,
    registers: [u8; 0x15],
}

impl Default for Attribute {
    fn default() -> Self {
        let mut registers = [0; 0x15];
        registers[..16].copy_from_slice(&text_mode::ATTRIBUTE_PALETTE);
        for &(index, value) in text_mode::ATTRIBUTE_REGISTERS {
            registers[usize::from(index)] = value;
        }
        Self {
            address: PALETTE_ADDRESS_SOURCE,
            data_next: false,
            registers,
        }
    }
}

impl Attribute {
    pub fn address(&self) -> u8 {
        self.address
    }

    pub fn register(&self, index: u8) -> u8 {
        self.registers[usize::from(index)]
    }

    pub fn read(&self) -> u8 {
        let index = self.address & 0x1f;
        if index <= COLOR_SELECT {
            self.register(index)
        } else {
            0xff
        }
    }

    /// Only an Input Status 1 read selects the address phase explicitly.
    pub fn reset_address(&mut self) {
        self.data_next = false;
    }

    pub fn write(&mut self, value: u8) {
        if !self.data_next {
            self.address = value & 0x3f;
        } else {
            let index = self.address & 0x1f;
            let mask = match index {
                0..=0x0f if self.address & PALETTE_ADDRESS_SOURCE == 0 => 0x3f,
                MODE_CONTROL => 0xef,
                OVERSCAN_COLOR => 0xff,
                COLOR_PLANE_ENABLE | HORIZONTAL_PANNING | COLOR_SELECT => 0x0f,
                _ => {
                    self.data_next = false;
                    return;
                }
            };
            self.registers[usize::from(index)] = value & mask;
        }
        self.data_next = !self.data_next;
    }

    pub fn output_enabled(&self) -> bool {
        self.address & PALETTE_ADDRESS_SOURCE != 0
    }

    pub fn blink_enabled(&self) -> bool {
        self.register(MODE_CONTROL) & 8 != 0
    }

    pub fn line_graphics(&self) -> bool {
        self.register(MODE_CONTROL) & 4 != 0
    }

    pub fn panning(&self, nine_dot_text: bool, split: bool) -> u8 {
        if split && self.register(MODE_CONTROL) & 0x20 != 0 {
            return 0;
        }
        let pan = self.register(HORIZONTAL_PANNING);
        if nine_dot_text {
            (pan + 1) % 9
        } else {
            pan & 7
        }
    }

    pub fn palette_index(&self, color: u8) -> u8 {
        let color = color & self.register(COLOR_PLANE_ENABLE);
        let palette = self.register(color);
        let select = self.register(COLOR_SELECT);
        let low = if self.register(MODE_CONTROL) & 0x80 != 0 {
            (palette & 0x0f) | ((select & 3) << 4)
        } else {
            palette
        };
        low | ((select & 0x0c) << 4)
    }
}

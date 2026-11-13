//! Graphics Controller registers, IBM VGA Function pages 2-78–2-88.

use super::text_mode;
use std::ops::Range;

pub const SET_RESET: u8 = 0x00;
pub const ENABLE_SET_RESET: u8 = 0x01;
pub const COLOR_COMPARE: u8 = 0x02;
pub const DATA_ROTATE: u8 = 0x03;
pub const READ_MAP_SELECT: u8 = 0x04;
pub const MODE: u8 = 0x05;
pub const MISCELLANEOUS: u8 = 0x06;
pub const COLOR_DONT_CARE: u8 = 0x07;
pub const BIT_MASK: u8 = 0x08;

/// Graphics Controller values and the current CPU register index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graphics {
    index: u8,
    pub values: [u8; 9],
}

impl Default for Graphics {
    fn default() -> Self {
        let mut values = [0; 9];
        for &(index, value) in text_mode::GRAPHICS_REGISTERS {
            values[usize::from(index)] = value;
        }
        Self { index: 0, values }
    }
}

impl Graphics {
    pub(crate) fn write_effect(&self) -> super::WriteKind {
        match self.index {
            MODE | MISCELLANEOUS => super::WriteKind::Display,
            _ => super::WriteKind::Access,
        }
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub(crate) fn select(&mut self, index: u8) {
        self.index = index & 0x0f;
    }

    fn register(&self, index: u8) -> u8 {
        self.values[usize::from(index)]
    }

    pub(crate) fn read(&self) -> u8 {
        if self.index <= BIT_MASK {
            self.register(self.index)
        } else {
            0xff
        }
    }

    pub(crate) fn write(&mut self, value: u8) -> bool {
        let mask = match self.index {
            SET_RESET | ENABLE_SET_RESET | COLOR_COMPARE | MISCELLANEOUS | COLOR_DONT_CARE => 0x0f,
            DATA_ROTATE => 0x1f,
            READ_MAP_SELECT => 0x03,
            MODE => 0x7b,
            BIT_MASK => 0xff,
            _ => return false,
        };
        let register = &mut self.values[usize::from(self.index)];
        let value = value & mask;
        let changed = *register != value;
        *register = value;
        changed
    }

    pub(crate) fn memory_window(&self) -> Range<u32> {
        let (start, size) = match (self.register(MISCELLANEOUS) >> 2) & 3 {
            0 => (0xa0000, 0x20000),
            1 => (0xa0000, 0x10000),
            2 => (0xb0000, 0x08000),
            _ => (0xb8000, 0x08000),
        };
        start..start + size
    }

    pub(crate) fn chain_odd_even(&self) -> bool {
        self.register(MISCELLANEOUS) & 2 != 0
    }

    pub(crate) fn read_controls(&self) -> ReadControls {
        ReadControls {
            plane: self.register(READ_MAP_SELECT),
            odd_even: self.register(MODE) & 0x10 != 0,
            compare: self.register(MODE) & 8 != 0,
            color: expand_planes(self.register(COLOR_COMPARE)),
            care: expand_planes(self.register(COLOR_DONT_CARE)),
        }
    }

    pub(crate) fn write_controls(&self) -> WriteControls {
        WriteControls {
            mode: self.register(MODE) & 3,
            rotate: u32::from(self.register(DATA_ROTATE) & 7),
            operation: (self.register(DATA_ROTATE) >> 3) & 3,
            mask: u32::from(self.register(BIT_MASK)) * 0x0101_0101,
            set_reset: expand_planes(self.register(SET_RESET)),
            enable_set_reset: expand_planes(self.register(ENABLE_SET_RESET)),
        }
    }
}

pub(crate) struct ReadControls {
    pub plane: u8,
    pub odd_even: bool,
    compare: bool,
    color: u32,
    care: u32,
}

impl ReadControls {
    pub fn compare(&self, latches: [u8; 4]) -> Option<u8> {
        if !self.compare {
            return None;
        }
        // A pixel matches only when every cared-for plane matches its color bit.
        let matches = !(u32::from_le_bytes(latches) ^ self.color) | !self.care;
        Some((matches & (matches >> 8) & (matches >> 16) & (matches >> 24)) as u8)
    }
}

/// Decoded when registers change. Each byte of a u32 is one plane, so the
/// graphics logic can operate on all four planes together.
pub(crate) struct WriteControls {
    mode: u8,
    rotate: u32,
    operation: u8,
    mask: u32,
    set_reset: u32,
    enable_set_reset: u32,
}

impl WriteControls {
    /// The CPU read latches supply both logical-operation input and masked bits.
    /// A write never reloads them from the destination address.
    #[inline]
    pub(crate) fn apply(&self, value: u8, latches: u32) -> u32 {
        if self.mode == 1 {
            return latches;
        }
        let rotated = u32::from(value.rotate_right(self.rotate)) * 0x0101_0101;
        let mask = if self.mode == 3 {
            self.mask & rotated
        } else {
            self.mask
        };
        let source = match self.mode {
            0 => (rotated & !self.enable_set_reset) | (self.set_reset & self.enable_set_reset),
            2 => expand_planes(value),
            _ => self.set_reset,
        };
        let combined = match self.operation {
            0 => source,
            1 => source & latches,
            2 => source | latches,
            _ => source ^ latches,
        };
        (combined & mask) | (latches & !mask)
    }
}

/// Expand each of the four plane bits to a full byte mask.
fn expand_planes(value: u8) -> u32 {
    u32::from_le_bytes([
        if value & 1 != 0 { 0xff } else { 0 },
        if value & 2 != 0 { 0xff } else { 0 },
        if value & 4 != 0 { 0xff } else { 0 },
        if value & 8 != 0 { 0xff } else { 0 },
    ])
}

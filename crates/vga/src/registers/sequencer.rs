//! Sequencer registers, IBM VGA Function pages 2-47–2-50.

use super::text_mode;

pub const RESET: u8 = 0x00;
pub const CLOCKING_MODE: u8 = 0x01;

/// Reset register value: hold synchronous reset, release asynchronous reset.
pub const SYNCHRONOUS_RESET: u8 = 0x01;
/// Reset register value: release both active-low reset inputs.
pub const RESET_RELEASED: u8 = 0x03;

pub(crate) struct Sequencer {
    index: u8,
    reset: u8,
    clocking_mode: u8,
}

impl Default for Sequencer {
    fn default() -> Self {
        Self {
            index: RESET,
            reset: RESET_RELEASED,
            clocking_mode: text_mode::CLOCKING_MODE,
        }
    }
}

impl Sequencer {
    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn select(&mut self, index: u8) {
        self.index = index & 7;
    }

    pub fn read(&self) -> u8 {
        match self.index {
            RESET => self.reset,
            CLOCKING_MODE => self.clocking_mode,
            _ => 0xff,
        }
    }

    pub fn write(&mut self, value: u8) {
        match self.index {
            RESET => self.reset = value & RESET_RELEASED,
            CLOCKING_MODE => self.clocking_mode = value & 0x3f,
            _ => {}
        }
    }

    pub fn reset_active(&self) -> bool {
        self.reset != RESET_RELEASED
    }

    pub fn dots_per_character(&self) -> u64 {
        // SR01 bit 0 selects eight dots when set, nine when clear.
        if self.clocking_mode & 1 == 0 {
            9
        } else {
            8
        }
    }

    pub fn dot_clock_divisor(&self) -> u64 {
        // SR01 bit 3 divides the oscillator by two before generating dot clocks.
        if self.clocking_mode & (1 << 3) == 0 {
            1
        } else {
            2
        }
    }
}

//! VGA port addresses, register definitions and the initial text-mode preset.
//! Firmware programs these through the device bus; mutable register state and
//! field decoding stay inside the VGA device.

pub mod crtc;
pub mod sequencer;
pub mod text_mode;

use crtc::Crtc;
use sequencer::Sequencer;
use std::ops::RangeInclusive;

pub const MONO_CRTC_INDEX_PORT: u16 = 0x3b4;
pub const MONO_CRTC_DATA_PORT: u16 = 0x3b5;
pub const MONO_INPUT_STATUS_1_PORT: u16 = 0x3ba;
pub const MISC_OUTPUT_WRITE_PORT: u16 = 0x3c2;
pub const SEQUENCER_INDEX_PORT: u16 = 0x3c4;
pub const SEQUENCER_DATA_PORT: u16 = 0x3c5;
pub const MISC_OUTPUT_READ_PORT: u16 = 0x3cc;
pub const CRTC_INDEX_PORT: u16 = 0x3d4;
pub const CRTC_DATA_PORT: u16 = 0x3d5;
pub const COLOR_INPUT_STATUS_1_PORT: u16 = 0x3da;
pub const CRTC_PORT_RANGE: RangeInclusive<u16> = CRTC_INDEX_PORT..=CRTC_DATA_PORT;

/// VGA register and status ports, including monochrome/color CRTC aliases.
pub const PORT_RANGES: [RangeInclusive<u16>; 6] = [
    MONO_CRTC_INDEX_PORT..=MONO_CRTC_DATA_PORT,
    MONO_INPUT_STATUS_1_PORT..=MONO_INPUT_STATUS_1_PORT,
    MISC_OUTPUT_WRITE_PORT..=SEQUENCER_DATA_PORT,
    MISC_OUTPUT_READ_PORT..=MISC_OUTPUT_READ_PORT,
    CRTC_PORT_RANGE,
    COLOR_INPUT_STATUS_1_PORT..=COLOR_INPUT_STATUS_1_PORT,
];

pub(super) struct Registers {
    pub crtc: Crtc,
    pub sequencer: Sequencer,
    miscellaneous: u8,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            crtc: Crtc::default(),
            sequencer: Sequencer::default(),
            miscellaneous: text_mode::MISC_OUTPUT,
        }
    }
}

impl Registers {
    pub fn status_port(&self) -> u16 {
        if self.miscellaneous & 1 == 0 {
            MONO_INPUT_STATUS_1_PORT
        } else {
            COLOR_INPUT_STATUS_1_PORT
        }
    }

    pub fn oscillator_hz(&self) -> u64 {
        // Miscellaneous Output bits 3:2 select the board's oscillator input
        // (IBM VGA Function p2-43), before the sequencer's dot-clock division.
        match (self.miscellaneous >> 2) & 3 {
            0 => 25_175_000,
            1 => 28_322_000,
            // No external oscillator is connected; selector 3 is reserved.
            _ => 0,
        }
    }

    fn crtc_ports(&self) -> (u16, u16) {
        if self.miscellaneous & 1 == 0 {
            (MONO_CRTC_INDEX_PORT, MONO_CRTC_DATA_PORT)
        } else {
            (CRTC_INDEX_PORT, CRTC_DATA_PORT)
        }
    }

    pub fn video_memory_enabled(&self) -> bool {
        // Miscellaneous Output bit 1 enables CPU access to display RAM.
        self.miscellaneous & (1 << 1) != 0
    }

    pub fn read_port(&self, port: u16) -> u8 {
        let (crtc_index, crtc_data) = self.crtc_ports();
        match port {
            MISC_OUTPUT_READ_PORT => self.miscellaneous,
            SEQUENCER_INDEX_PORT => self.sequencer.index(),
            SEQUENCER_DATA_PORT => self.sequencer.read(),
            port if port == crtc_index => self.crtc.index(),
            port if port == crtc_data => self.crtc.read(),
            _ => 0xff,
        }
    }

    pub fn write_port(&mut self, port: u16, value: u8) {
        let (crtc_index, crtc_data) = self.crtc_ports();
        match port {
            MISC_OUTPUT_WRITE_PORT => self.miscellaneous = value,
            SEQUENCER_INDEX_PORT => self.sequencer.select(value),
            SEQUENCER_DATA_PORT => self.sequencer.write(value),
            port if port == crtc_index => self.crtc.select(value),
            port if port == crtc_data => self.crtc.write(value),
            _ => {}
        }
    }
}

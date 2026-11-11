//! VGA register banks and port access. Each bank owns its raw values and CPU
//! access state; display captures copy display inputs without reading ports.

pub mod attribute;
pub mod crtc;
pub mod dac;
pub mod dispi;
pub mod graphics;
pub mod sequencer;
pub(crate) mod setup;
pub mod text_mode;

use attribute::Attribute;
use crtc::Crtc;
use dac::Dac;
use graphics::Graphics;
use sequencer::Sequencer;
use std::ops::RangeInclusive;

pub use attribute::{
    ADDRESS_PORT as ATTRIBUTE_ADDRESS_PORT, DATA_READ_PORT as ATTRIBUTE_DATA_READ_PORT,
};
pub use crtc::{
    COLOR_DATA_PORT as CRTC_DATA_PORT, COLOR_INDEX_PORT as CRTC_INDEX_PORT,
    MONO_DATA_PORT as MONO_CRTC_DATA_PORT, MONO_INDEX_PORT as MONO_CRTC_INDEX_PORT,
};
pub use dac::{
    DATA_PORT as DAC_DATA_PORT, MASK_PORT as DAC_MASK_PORT,
    READ_ADDRESS_PORT as DAC_READ_ADDRESS_PORT, WRITE_ADDRESS_PORT as DAC_WRITE_ADDRESS_PORT,
};
pub use graphics::{DATA_PORT as GRAPHICS_DATA_PORT, INDEX_PORT as GRAPHICS_INDEX_PORT};
pub use sequencer::{DATA_PORT as SEQUENCER_DATA_PORT, INDEX_PORT as SEQUENCER_INDEX_PORT};
pub use setup::{ADAPTER_CONTROL_PORT, OPTION_SELECT_PORT};

pub const MONO_INPUT_STATUS_1_PORT: u16 = 0x3ba;
pub const MISC_OUTPUT_WRITE_PORT: u16 = 0x3c2;
pub const FEATURE_CONTROL_READ_PORT: u16 = 0x3ca;
pub const MISC_OUTPUT_READ_PORT: u16 = 0x3cc;
pub const COLOR_INPUT_STATUS_1_PORT: u16 = 0x3da;
pub const CRTC_PORT_RANGE: RangeInclusive<u16> = CRTC_INDEX_PORT..=CRTC_DATA_PORT;

/// VGA register, status, setup and DISPI ports, including monochrome/color aliases.
pub const PORT_RANGES: [RangeInclusive<u16>; 8] = [
    dispi::PORT_RANGE,
    OPTION_SELECT_PORT..=OPTION_SELECT_PORT,
    MONO_CRTC_INDEX_PORT..=MONO_CRTC_DATA_PORT,
    MONO_INPUT_STATUS_1_PORT..=MONO_INPUT_STATUS_1_PORT,
    ATTRIBUTE_ADDRESS_PORT..=GRAPHICS_DATA_PORT,
    CRTC_PORT_RANGE,
    COLOR_INPUT_STATUS_1_PORT..=COLOR_INPUT_STATUS_1_PORT,
    ADAPTER_CONTROL_PORT..=ADAPTER_CONTROL_PORT,
];

/// An independent copy of the VGA register banks and CPU access state.
/// Live port bindings stay with the device's components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registers {
    pub crtc: crtc::State,
    pub sequencer: sequencer::State,
    pub graphics: graphics::State,
    pub attribute: attribute::State,
    pub dac: dac::State,
    pub miscellaneous: u8,
    pub feature_control: u8,
}

/// Live VGA-compatible programming, including the DAC shared with packed modes.
/// Keeping them together lets memory access borrow their state independently
/// of mutable VRAM. Each controller owns its own bindings and programming rules.
pub(crate) struct LegacyRegisters {
    pub crtc: Crtc,
    pub sequencer: Sequencer,
    pub graphics: Graphics,
    pub attribute: Attribute,
    pub dac: Dac,
    pub miscellaneous: u8,
    pub feature_control: u8,
}

impl Default for LegacyRegisters {
    fn default() -> Self {
        Self {
            crtc: Crtc::default(),
            sequencer: Sequencer::default(),
            graphics: Graphics::default(),
            attribute: Attribute::default(),
            dac: Dac::default(),
            miscellaneous: text_mode::MISC_OUTPUT,
            feature_control: 0,
        }
    }
}

impl LegacyRegisters {
    pub fn snapshot(&self) -> Registers {
        Registers {
            crtc: self.crtc.state.clone(),
            sequencer: self.sequencer.state.clone(),
            graphics: self.graphics.state.clone(),
            attribute: self.attribute.state.clone(),
            dac: self.dac.state.clone(),
            miscellaneous: self.miscellaneous,
            feature_control: self.feature_control,
        }
    }

    pub(crate) fn status_port(&self) -> u16 {
        if self.miscellaneous & 1 == 0 {
            MONO_INPUT_STATUS_1_PORT
        } else {
            COLOR_INPUT_STATUS_1_PORT
        }
    }

    pub(crate) fn oscillator_hz(&self) -> u64 {
        // IBM VGA has two on-board oscillators. The external input (2) is
        // disconnected here; the reserved selection (3) also supplies no clock.
        match (self.miscellaneous >> 2) & 3 {
            0 => 25_175_000,
            1 => 28_322_000,
            _ => 0,
        }
    }

    pub(crate) fn video_memory_enabled(&self) -> bool {
        // Miscellaneous Output bit 1 enables CPU access to display RAM.
        self.miscellaneous & (1 << 1) != 0
    }
}

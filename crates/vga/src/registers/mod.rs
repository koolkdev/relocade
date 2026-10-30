//! VGA register banks and port access. Each bank owns its raw values and CPU
//! access state; display captures clone the same banks without reading ports.

pub mod attribute;
pub mod crtc;
pub mod dac;
pub mod graphics;
pub mod sequencer;
pub mod text_mode;

use attribute::Attribute;
use crtc::Crtc;
use dac::Dac;
use graphics::Graphics;
use sequencer::Sequencer;
use std::ops::RangeInclusive;

pub const MONO_CRTC_INDEX_PORT: u16 = 0x3b4;
pub const MONO_CRTC_DATA_PORT: u16 = 0x3b5;
pub const MONO_INPUT_STATUS_1_PORT: u16 = 0x3ba;
pub const ATTRIBUTE_ADDRESS_PORT: u16 = 0x3c0;
pub const ATTRIBUTE_DATA_READ_PORT: u16 = 0x3c1;
pub const MISC_OUTPUT_WRITE_PORT: u16 = 0x3c2;
pub const SEQUENCER_INDEX_PORT: u16 = 0x3c4;
pub const SEQUENCER_DATA_PORT: u16 = 0x3c5;
pub const DAC_MASK_PORT: u16 = 0x3c6;
pub const DAC_READ_ADDRESS_PORT: u16 = 0x3c7;
pub const DAC_WRITE_ADDRESS_PORT: u16 = 0x3c8;
pub const DAC_DATA_PORT: u16 = 0x3c9;
pub const FEATURE_CONTROL_READ_PORT: u16 = 0x3ca;
pub const MISC_OUTPUT_READ_PORT: u16 = 0x3cc;
pub const GRAPHICS_INDEX_PORT: u16 = 0x3ce;
pub const GRAPHICS_DATA_PORT: u16 = 0x3cf;
pub const CRTC_INDEX_PORT: u16 = 0x3d4;
pub const CRTC_DATA_PORT: u16 = 0x3d5;
pub const COLOR_INPUT_STATUS_1_PORT: u16 = 0x3da;
pub const CRTC_PORT_RANGE: RangeInclusive<u16> = CRTC_INDEX_PORT..=CRTC_DATA_PORT;

/// VGA register and status ports, including monochrome/color CRTC aliases.
pub const PORT_RANGES: [RangeInclusive<u16>; 5] = [
    MONO_CRTC_INDEX_PORT..=MONO_CRTC_DATA_PORT,
    MONO_INPUT_STATUS_1_PORT..=MONO_INPUT_STATUS_1_PORT,
    ATTRIBUTE_ADDRESS_PORT..=GRAPHICS_DATA_PORT,
    CRTC_PORT_RANGE,
    COLOR_INPUT_STATUS_1_PORT..=COLOR_INPUT_STATUS_1_PORT,
];

/// Complete stored VGA register state, used by the device and its captures.
/// Bank indices and transfer phases are retained alongside the raw values;
/// cloning this state never advances a CPU access or evaluates display settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registers {
    pub crtc: Crtc,
    pub sequencer: Sequencer,
    pub graphics: Graphics,
    pub attribute: Attribute,
    pub dac: Dac,
    pub miscellaneous: u8,
    pub feature_control: u8,
}

impl Default for Registers {
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

impl Registers {
    pub(crate) fn status_port(&self) -> u16 {
        if self.miscellaneous & 1 == 0 {
            MONO_INPUT_STATUS_1_PORT
        } else {
            COLOR_INPUT_STATUS_1_PORT
        }
    }

    fn crtc_ports(&self) -> (u16, u16) {
        if self.miscellaneous & 1 == 0 {
            (MONO_CRTC_INDEX_PORT, MONO_CRTC_DATA_PORT)
        } else {
            (CRTC_INDEX_PORT, CRTC_DATA_PORT)
        }
    }

    pub(crate) fn video_memory_enabled(&self) -> bool {
        // Miscellaneous Output bit 1 enables CPU access to display RAM.
        self.miscellaneous & (1 << 1) != 0
    }

    pub(crate) fn read_port(&mut self, port: u16, raster_status: u8) -> u8 {
        let (crtc_index, crtc_data) = self.crtc_ports();
        match port {
            ATTRIBUTE_ADDRESS_PORT => self.attribute.address(),
            ATTRIBUTE_DATA_READ_PORT => self.attribute.read(),
            MISC_OUTPUT_WRITE_PORT => 0x10, // Attached color display; no retrace IRQ wired.
            MISC_OUTPUT_READ_PORT => self.miscellaneous,
            SEQUENCER_INDEX_PORT => self.sequencer.index(),
            SEQUENCER_DATA_PORT => self.sequencer.read(),
            GRAPHICS_INDEX_PORT => self.graphics.index(),
            GRAPHICS_DATA_PORT => self.graphics.read(),
            DAC_MASK_PORT => self.dac.mask,
            DAC_READ_ADDRESS_PORT => self.dac.state(),
            DAC_WRITE_ADDRESS_PORT => self.dac.address(),
            DAC_DATA_PORT => self.dac.read(),
            FEATURE_CONTROL_READ_PORT => self.feature_control,
            port if port == self.status_port() => {
                self.attribute.reset_address();
                raster_status
            }
            port if port == crtc_index => self.crtc.index(),
            port if port == crtc_data => self.crtc.read(),
            _ => 0xff,
        }
    }

    pub(crate) fn write_port(&mut self, port: u16, value: u8) {
        let (crtc_index, crtc_data) = self.crtc_ports();
        match port {
            ATTRIBUTE_ADDRESS_PORT => self.attribute.write(value),
            MISC_OUTPUT_WRITE_PORT => self.miscellaneous = value,
            SEQUENCER_INDEX_PORT => self.sequencer.select(value),
            SEQUENCER_DATA_PORT => self.sequencer.write(value),
            GRAPHICS_INDEX_PORT => self.graphics.select(value),
            GRAPHICS_DATA_PORT => self.graphics.write(value),
            DAC_MASK_PORT => self.dac.mask = value,
            DAC_READ_ADDRESS_PORT => self.dac.select(value, true),
            DAC_WRITE_ADDRESS_PORT => self.dac.select(value, false),
            DAC_DATA_PORT => self.dac.write(value),
            port if port == self.status_port() => self.feature_control = value,
            port if port == crtc_index => self.crtc.select(value),
            port if port == crtc_data => self.crtc.write(value),
            _ => {}
        }
    }
}

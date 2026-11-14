//! Captured VGA registers, prepared display parameters and live latches.
use super::{VgaDisplayLayout, VgaParameters};
use crate::{
    registers::{crtc::CrtcRegisters, graphics, sequencer},
    DisplayGeometry, Registers,
};
use std::sync::Arc;

/// Captured VGA programming, prepared display parameters and live latches.
/// Raw banks support inspection; drawing uses the parameters prepared by the card.
/// CPU register indices, transfer phases and the shared DAC are separate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VgaDisplayState {
    pub sequencer: [u8; 5],
    pub crtc: CrtcRegisters,
    pub graphics: [u8; 9],
    pub attributes: [u8; 0x15],
    pub miscellaneous: u8,
    pub display_enabled: bool,
    /// Start address latched at the last serviced VGA retrace.
    pub display_start: u32,
    pub layout: VgaDisplayLayout,
    pub(crate) parameters: Arc<VgaParameters>,
    /// Cursor phase toggles every eight retraces.
    pub cursor_blink_on: bool,
    /// Text and graphics attribute phase toggles every sixteen retraces.
    pub attribute_blink_on: bool,
}

impl VgaDisplayState {
    pub(crate) fn from_registers(
        registers: &Registers,
        layout: VgaDisplayLayout,
        parameters: Arc<VgaParameters>,
        display_start: u32,
    ) -> Self {
        Self {
            sequencer: registers.sequencer.values,
            crtc: registers.crtc.registers,
            graphics: registers.graphics.values,
            attributes: registers.attribute.values,
            miscellaneous: registers.miscellaneous,
            display_enabled: registers.attribute.display_enabled(),
            display_start,
            layout,
            parameters,
            cursor_blink_on: true,
            attribute_blink_on: true,
        }
    }

    /// Apply retrace-driven phases without decoding display programming again.
    pub(crate) fn with_blink_phase(mut self, retrace_count: u64) -> Self {
        // IBM VGA: cursor VSYNC/16 and attribute VSYNC/32, both 50% duty.
        self.cursor_blink_on = (retrace_count / 8).is_multiple_of(2);
        self.attribute_blink_on = (retrace_count / 16).is_multiple_of(2);
        self
    }

    /// Compare all captured state except known CPU-only controls. Normalizing
    /// copies preserves the raw snapshots and their public equality.
    pub(super) fn same_inputs(&self, other: &Self) -> bool {
        self.clone().display_inputs() == other.clone().display_inputs()
    }

    fn display_inputs(mut self) -> Self {
        self.sequencer[sequencer::MAP_MASK as usize] = 0;
        // Exclude chain-four and CPU odd/even; retain extended-font addressing.
        self.sequencer[sequencer::MEMORY_MODE as usize] &= !0x0c;
        for index in [
            graphics::SET_RESET,
            graphics::ENABLE_SET_RESET,
            graphics::COLOR_COMPARE,
            graphics::DATA_ROTATE,
            graphics::READ_MAP_SELECT,
            graphics::COLOR_DONT_CARE,
            graphics::BIT_MASK,
        ] {
            self.graphics[index as usize] = 0;
        }
        self.graphics[graphics::MODE as usize] &= !0x1b; // CPU read/write modes.
        self.graphics[graphics::MISCELLANEOUS as usize] &= !0x0e; // CPU aperture and odd/even.
        self
    }
}

/// VGA row interpretation accompanying the common output geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VgaGeometry {
    pub output: DisplayGeometry,
    /// Physical scanlines represented by each image row.
    pub scanline_step: u32,
}

impl VgaDisplayState {
    pub fn geometry(&self) -> VgaGeometry {
        self.parameters.geometry
    }
}

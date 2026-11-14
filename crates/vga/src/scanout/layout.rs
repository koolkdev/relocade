//! Effective VGA display inputs prepared when display controls change.
use crate::{memory::VGA_MEMORY_BYTES, registers::crtc, Registers};

/// Card-supplied bounds and counters for the shared text/planar scanout.
/// The display start is separate because it is latched at retrace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VgaDisplayLayout {
    /// Power-of-two physical memory extent, with four interleaved planes.
    pub memory_bytes: u32,
    /// Address consecutive plane groups instead of rotating IBM CRTC pins.
    pub linear_addressing: bool,
    pub cursor_address: u32,
    /// Visible height and split position in vertical-counter units.
    pub vertical_display: u32,
    pub line_compare: u32,
    /// Half-open blanking intervals in vertical-counter units.
    pub vertical_blanking: [[u32; 2]; 2],
}

impl VgaDisplayLayout {
    pub(crate) fn vga(registers: &Registers) -> Self {
        let crtc = &registers.crtc.registers;
        let values = &crtc.values;
        Self {
            memory_bytes: VGA_MEMORY_BYTES,
            linear_addressing: false,
            cursor_address: u32::from(u16::from_be_bytes([
                values[crtc::CURSOR_ADDRESS_HIGH as usize],
                values[crtc::CURSOR_ADDRESS_LOW as usize],
            ])),
            vertical_display: crtc.vertical_display() as u32,
            line_compare: u32::from(values[crtc::LINE_COMPARE as usize])
                | (u32::from(values[crtc::OVERFLOW as usize] & 0x10) << 4)
                | (u32::from(values[crtc::MAXIMUM_SCAN_LINE as usize] & 0x40) << 3),
            vertical_blanking: crtc
                .vertical_blanking()
                .map(|range| [range.start, range.end]),
        }
    }

    pub(crate) fn plane_address_mask(&self) -> u32 {
        self.memory_bytes / 4 - 1
    }
}

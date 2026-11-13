//! EV8913 clock inputs and ET4000 vertical extensions supplied to the shared raster.
use super::{
    registers::{Extensions, AUXILIARY, COMPATIBILITY, OVERFLOW},
    Et4000Ax, PIXEL_CLOCKS,
};
use crate::{Timing, VgaCore};

impl Et4000Ax {
    pub(super) fn display_timing(vga: &VgaCore, extensions: &Extensions) -> Timing {
        let clock = (usize::from(extensions.crtc(COMPATIBILITY).unwrap() & 2) << 1)
            | usize::from((vga.registers.miscellaneous >> 2) & 3);
        let auxiliary = extensions.sequencer(AUXILIARY).unwrap();
        let divisor = if auxiliary & 1 != 0 {
            4
        } else if auxiliary & 0x40 != 0 {
            2
        } else {
            1
        };
        let crtc = &vga.registers.crtc.registers;
        let overflow = extensions.crtc(OVERFLOW).unwrap();
        Timing::vga_clocked(&vga.registers, PIXEL_CLOCKS[clock])
            .divide_clock(divisor)
            .with_vertical_counts(
                crtc.vertical_total() + (u64::from(overflow & 2) << 9),
                crtc.vertical_display() + (u64::from(overflow & 4) << 8),
                crtc.vertical_retrace_start() + (u64::from(overflow & 8) << 7),
            )
            .with_interlace(overflow & 0x80 != 0)
    }
}

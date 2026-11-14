//! Register-derived image addressing, independent of the CPU segment pointers.
use super::Et4000Ax;
use crate::{
    registers::{attribute, graphics},
    VgaDisplayState,
};

impl Et4000Ax {
    pub(super) fn vga_display_state(&self) -> VgaDisplayState {
        let mut state =
            VgaDisplayState::capture(&self.vga.registers, self.output.raster.retrace_count);
        // Keep VGA's serializer, row repeats, split screen and blanking in its
        // decoder. ET4000's linear-byte memory organization changes addressing.
        state.linear_addressing = state.attributes[attribute::MODE_CONTROL as usize] & 0x41 == 0x41
            && state.graphics[graphics::MODE as usize] & 0x40 != 0;
        state
    }
}

use crate::{DispiAdapter, Framebuffer, FramebufferAccess, Scanout, VgaDisplayState};

impl FramebufferAccess for DispiAdapter {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let scanout = match self.dispi.display_layout() {
            Some(layout) => Scanout::Packed(layout),
            None => {
                let mut state = VgaDisplayState::capture(&self.vga.registers, 0);
                // Services draw into the programmed viewport, including a page
                // selected for the next retrace. Captures retain their latched start.
                state.display_start = u16::from_be_bytes([
                    state.crtc.values[crate::registers::crtc::START_ADDRESS_HIGH as usize],
                    state.crtc.values[crate::registers::crtc::START_ADDRESS_LOW as usize],
                ]);
                Scanout::Vga(state)
            }
        };
        Framebuffer {
            memory: &mut self.memory,
            scanout,
        }
    }
}

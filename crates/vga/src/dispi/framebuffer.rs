use crate::{DispiAdapter, Framebuffer, FramebufferAccess, Scanout, VgaDisplayState};

impl FramebufferAccess for DispiAdapter {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let scanout = match self.dispi.display_layout() {
            Some(layout) => Scanout::Packed(layout),
            None => Scanout::Vga(VgaDisplayState::for_drawing(&self.vga.registers)),
        };
        Framebuffer {
            memory: &mut self.memory,
            scanout,
        }
    }
}

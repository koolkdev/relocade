use crate::{DispiAdapter, Framebuffer, FramebufferAccess};

impl FramebufferAccess for DispiAdapter {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let crtc = &self.vga.registers.crtc.registers.values;
        let scanout = self.scanout_at(u16::from_be_bytes([crtc[0x0c], crtc[0x0d]]));
        Framebuffer {
            memory: &mut self.memory,
            scanout,
        }
    }
}

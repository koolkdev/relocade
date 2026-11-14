use crate::{DispiAdapter, Framebuffer, FramebufferAccess};

impl FramebufferAccess for DispiAdapter {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let crtc = &self.vga.registers.crtc.registers.values;
        let scanout = self.output.display.scanout_at(
            &self.vga.registers,
            u32::from(u16::from_be_bytes([crtc[0x0c], crtc[0x0d]])),
            self.output.raster.retrace_count,
        );
        Framebuffer {
            memory: &mut self.memory,
            scanout,
        }
    }
}

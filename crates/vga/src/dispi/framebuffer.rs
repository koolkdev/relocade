use crate::{DispiAdapter, Framebuffer, FramebufferAccess};

impl FramebufferAccess for DispiAdapter {
    fn framebuffer(&mut self) -> Framebuffer<'_> {
        self.capture_if_due();
        let scanout = self.output.display.scanout_at(
            &self.vga.registers,
            u32::from(self.vga.registers.crtc.programmed_start()),
            self.output.raster.retrace_count,
        );
        Framebuffer {
            memory: &mut self.memory,
            scanout,
        }
    }
}

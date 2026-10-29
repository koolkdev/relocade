use relocade_renderer::{fit_image, Rect, Renderer};
use relocade_vga::{FrameSource, VgaImage, VgaRenderer};

/// DOS-specific drawing state on the harness graphics owner.
pub struct Graphics {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    vga: VgaRenderer,
    frame: Option<VgaImage>,
}
impl Graphics {
    pub(crate) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        let renderer = Renderer::new(device.clone(), format).map_err(|error| error.to_string())?;
        let vga = VgaRenderer::new(&device, &queue).map_err(|error| error.to_string())?;
        Ok(Self {
            device,
            queue,
            renderer,
            vga,
            frame: None,
        })
    }
    pub(crate) fn draw(
        &mut self,
        source: &FrameSource,
        attachment: &wgpu::TextureView,
        size: [u32; 2],
    ) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let updated = source
            .current_capture()
            .filter(|capture| {
                self.frame
                    .as_ref()
                    .is_none_or(|frame| frame.sequence != capture.sequence)
            })
            .map(|capture| self.vga.encode(&self.queue, &mut encoder, &capture));
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("DOS viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: attachment,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let Some(frame) = updated.as_ref().or(self.frame.as_ref()) {
                let bounds = Rect::from_size(size);
                self.renderer
                    .draw_image(&mut pass, &frame.image, fit_image(&frame.image, bounds));
            }
        }
        self.queue.submit([encoder.finish()]);
        if updated.is_some() {
            self.frame = updated;
        }
    }
}

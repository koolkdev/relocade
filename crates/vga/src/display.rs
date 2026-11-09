use crate::{CapturedFrame, FrameSource, VgaRenderer};
use relocade_frontend::display::DisplayRenderer;
use relocade_renderer::{FrameError, Image};
use std::sync::Arc;

/// Connects captured VGA output to a graphics host. Construct this on the graphics
/// owner using its device; the execution owner supplies only the frame source.
/// `service` receives captures already synchronized by the VGA task and uploads
/// their changes. Host redraws independently decode the latest uploaded state
/// when needed and reuse the existing image while awaiting another capture.
pub struct VgaDisplay {
    source: FrameSource,
    renderer: Option<VgaRenderer>,
    capture: Option<Arc<CapturedFrame>>,
    needs_decode: bool,
    image: Option<Image>,
}

impl VgaDisplay {
    /// The adapter retains the source across GPU loss, including after capture
    /// production ends. Host device replacement does not change guest VGA state.
    pub fn new(device: &wgpu::Device, source: FrameSource) -> Result<Self, FrameError> {
        Ok(Self {
            source,
            renderer: Some(VgaRenderer::new(device)?),
            capture: None,
            needs_decode: false,
            image: None,
        })
    }

    /// Image from the most recent decode. Redraws with unchanged rendering inputs
    /// reuse it. Submit recorded commands before consuming the pixels.
    /// Storage is reused across decodes and does not retain historical pixels.
    pub fn current_image(&self) -> Option<&Image> {
        self.image.as_ref()
    }
}

impl DisplayRenderer for VgaDisplay {
    fn device_lost(&mut self) {
        self.image = None;
        self.renderer = None;
    }

    fn attach_device(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), String> {
        let mut renderer = VgaRenderer::new(device).map_err(|error| error.to_string())?;
        // Restore the last uploaded capture independently of new publications.
        if let Some(capture) = &self.capture {
            renderer.upload(queue, &capture.display);
            queue.submit([]);
        }
        self.renderer = Some(renderer);
        self.image = None;
        self.needs_decode = true;
        Ok(())
    }

    async fn service(&mut self, queue: &wgpu::Queue) -> Option<bool> {
        self.source
            .next_capture(self.capture.as_ref().map(|capture| capture.sequence))
            .await?;
        let capture = self.source.take_capture()?;
        self.needs_decode |= self.renderer.as_mut()?.upload(queue, &capture.display);
        queue.submit([]);
        self.capture = Some(capture);
        Some(self.needs_decode || self.image.is_none())
    }

    fn encode_image(
        &mut self,
        _queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Option<&Image> {
        let renderer = self.renderer.as_mut()?;
        if self.needs_decode || self.image.is_none() {
            self.image = renderer.encode(encoder);
            self.needs_decode = false;
        }
        self.image.as_ref()
    }
}

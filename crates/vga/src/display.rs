use crate::{DisplaySnapshot, VgaRenderer};
use async_channel::Receiver;
use relocade_frontend::display::DisplayRenderer;
use relocade_renderer::{FrameError, Image};

/// Connects captured VGA output to a graphics host. Construct this on the graphics
/// owner using its device; the execution owner supplies only the snapshot receiver.
/// `service` receives captures already synchronized by the VGA task and uploads
/// their changes. Host redraws independently decode the latest uploaded state
/// when needed and reuse the existing image while awaiting another capture.
pub struct VgaDisplay {
    frames: Receiver<DisplaySnapshot>,
    renderer: Option<VgaRenderer>,
    snapshot: Option<DisplaySnapshot>,
    needs_decode: bool,
    image: Option<Image>,
}

impl VgaDisplay {
    /// The adapter retains the source across GPU loss, including after capture
    /// production ends. Host device replacement does not change guest VGA state.
    pub fn new(
        device: &wgpu::Device,
        frames: Receiver<DisplaySnapshot>,
    ) -> Result<Self, FrameError> {
        Ok(Self {
            frames,
            renderer: Some(VgaRenderer::new(device)?),
            snapshot: None,
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
        if let Some(snapshot) = &self.snapshot {
            renderer.upload(queue, snapshot);
            queue.submit([]);
        }
        self.renderer = Some(renderer);
        self.image = None;
        self.needs_decode = true;
        Ok(())
    }

    async fn service(&mut self, queue: &wgpu::Queue) -> Option<bool> {
        let snapshot = self.frames.recv().await.ok()?;
        self.needs_decode |= self.renderer.as_mut()?.upload(queue, &snapshot);
        queue.submit([]);
        self.snapshot = Some(snapshot);
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

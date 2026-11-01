use crate::{FrameSource, VgaImage, VgaRenderer};
use relocade_frontend::display::DisplayRenderer;
use relocade_renderer::{FrameError, Image};

/// Connects captured VGA output to a graphics host. Construct this on the graphics
/// owner using its device; the execution owner supplies only the frame source.
/// `service` observes changed captures without GPU work. On a requested redraw,
/// `encode_image` takes the latest capture with its accumulated dirty pages,
/// uploads it and records decoding if needed.
/// The host submits uploads, decoding and drawing together, then presents.
pub struct VgaDisplay {
    source: FrameSource,
    renderer: Option<VgaRenderer>,
    observed_sequence: Option<u64>,
    frame: Option<VgaImage>,
}

impl VgaDisplay {
    /// The adapter retains the source across GPU loss, including after capture
    /// production ends. Host device replacement does not change guest VGA state.
    pub fn new(device: &wgpu::Device, source: FrameSource) -> Result<Self, FrameError> {
        Ok(Self {
            source,
            renderer: Some(VgaRenderer::new(device)?),
            observed_sequence: None,
            frame: None,
        })
    }

    /// Image and source metadata from the most recent decode. Redraws with
    /// unchanged rendering inputs leave both intact. Submit recorded commands
    /// before consuming the pixels.
    /// Storage is reusable, not a historical frame.
    pub fn current_frame(&self) -> Option<&VgaImage> {
        self.frame.as_ref()
    }
}

impl DisplayRenderer for VgaDisplay {
    fn device_lost(&mut self) {
        self.frame = None;
        self.renderer = None;
    }

    fn attach_device(&mut self, device: &wgpu::Device, _queue: &wgpu::Queue) -> Result<(), String> {
        // Fresh storage forces a full upload on the next redraw, even when the
        // source sequence did not change or capture production has already ended.
        self.renderer = Some(VgaRenderer::new(device).map_err(|error| error.to_string())?);
        self.frame = None;
        Ok(())
    }

    async fn service(&mut self, _queue: &wgpu::Queue) -> Option<bool> {
        let capture = self.source.next_capture(self.observed_sequence).await?;
        self.observed_sequence = Some(capture.sequence);
        Some(true)
    }

    fn encode_image(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Option<&Image> {
        let renderer = self.renderer.as_mut()?;
        let capture = self.source.take_capture()?;
        let changed = renderer.upload(queue, &capture);
        self.observed_sequence = Some(capture.sequence);
        if changed || self.frame.is_none() {
            self.frame = renderer.encode(encoder);
        }
        self.frame.as_ref().map(|frame| &frame.image)
    }
}

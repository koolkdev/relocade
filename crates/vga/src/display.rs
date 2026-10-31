use crate::{FrameSource, VgaImage, VgaRenderer};
use relocade_frontend::display::DisplayRenderer;
use relocade_renderer::{FrameError, Image};

/// Connects captured VGA output to a graphics host. Construct this on the graphics
/// owner using its device; the execution owner supplies only the frame source.
/// Input uploads continue independently of redraw demand. `encode_image` records
/// decoding when rendering inputs change. The host submits and presents.
pub struct VgaDisplay {
    source: FrameSource,
    renderer: Option<VgaRenderer>,
    uploaded_sequence: Option<u64>,
    needs_decode: bool,
    frame: Option<VgaImage>,
}

impl VgaDisplay {
    /// The adapter retains the source across GPU loss, including after capture
    /// production ends. Host device replacement does not change guest VGA state.
    pub fn new(device: &wgpu::Device, source: FrameSource) -> Result<Self, FrameError> {
        Ok(Self {
            source,
            renderer: Some(VgaRenderer::new(device)?),
            uploaded_sequence: None,
            needs_decode: false,
            frame: None,
        })
    }

    /// Image and source metadata from the most recent decode. Unchanged captures
    /// leave both intact. Submit recorded commands before consuming the pixels.
    /// Storage is reusable, not a historical frame.
    pub fn current_frame(&self) -> Option<&VgaImage> {
        self.frame.as_ref()
    }
}

impl DisplayRenderer for VgaDisplay {
    fn device_lost(&mut self) {
        self.frame = None;
        self.renderer = None;
        self.uploaded_sequence = None;
        self.needs_decode = false;
    }

    fn attach_device(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), String> {
        let mut renderer = VgaRenderer::new(device).map_err(|error| error.to_string())?;
        let capture = self.source.current_capture();
        if let Some(capture) = &capture {
            // Fresh GPU storage requires a full upload, even when the source
            // sequence did not change or capture production has already ended.
            renderer.upload(queue, capture);
        }
        self.uploaded_sequence = capture.map(|capture| capture.sequence);
        self.needs_decode = self.uploaded_sequence.is_some();
        self.frame = None;
        self.renderer = Some(renderer);
        Ok(())
    }

    async fn update(&mut self, queue: &wgpu::Queue) -> Option<bool> {
        let capture = self.source.next_capture(self.uploaded_sequence).await?;
        let changed = self
            .renderer
            .as_mut()
            .expect("host attaches a device before updating")
            .upload(queue, &capture);
        self.uploaded_sequence = Some(capture.sequence);
        self.needs_decode |= changed;
        Some(changed)
    }

    fn encode_image(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<&Image> {
        if self.needs_decode {
            self.frame = self.renderer.as_mut()?.encode(encoder);
            self.needs_decode = false;
        }
        self.frame.as_ref().map(|frame| &frame.image)
    }
}

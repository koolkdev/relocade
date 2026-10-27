use crate::Texture;
use std::future::Future;

/// GPU resources shared by rendering and window presentation on one worker.
#[derive(Clone)]
pub struct Graphics {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Owns application graphics state and its current display texture.
/// Poll its work independently of host redraws, including while hidden.
pub trait Renderer: Future<Output = Result<(), String>> + Sized + 'static {
    /// Sendable launch data, including any application-specific request channel.
    type Source: Send + 'static;

    /// Constructs state on the graphics worker; the renderer need not be Send or
    /// Unpin. Keep each poll bounded so viewing and cancellation can progress.
    /// Native readbacks must drive device polling independently of host frames.
    fn new(source: Self::Source, graphics: Graphics) -> Result<Self, String>;

    /// The texture to display, or None until one is available. It must use this
    /// renderer's device. Submit updates before the viewer samples it through
    /// the shared queue. Contents remain live; clones do not freeze a frame.
    /// The renderer can replace the texture, including to change dimensions.
    /// After successful completion, an embedding can retain its final texture
    /// before dropping the renderer. Do not poll a completed renderer again.
    fn texture(&self) -> Option<&Texture>;
}

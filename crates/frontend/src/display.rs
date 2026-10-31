use relocade_renderer::Image;
use std::future::{ready, Future};

/// Graphics-side implementation of a display connection. It consumes its source
/// and generates an image using the host's GPU resources. This is separate from
/// the emulated display device and the shared renderer's image-drawing operations.
/// It owns neither a surface nor a worker, and never submits commands.
/// The connection survives host device replacement. Its implementation owns
/// GPU invalidation and any guest-visible loss/reset behavior.
pub trait DisplayRenderer {
    /// The host has stopped using its device and will discard its GPU resources.
    /// Invalidate device-bound state and notify the guest if the backend requires
    /// it. The logical connection stays alive. This also precedes a replacement
    /// prompted by surface loss, even without a device-loss callback.
    /// No `update` or `encode_image` runs until replacement succeeds.
    fn device_lost(&mut self) {}

    /// Attaches a replacement host device after `device_lost`. Returning `Ok`
    /// allows updates and drawing to resume; it does not imply guest recovery.
    /// A backend may keep returning no image while processing a guest reset via
    /// `update`. Waiting for a guest reset keeps updates pending, not closed.
    /// Return an error only when this connection cannot continue.
    /// The host submits queued uploads before another `update` or `encode_image`.
    /// By default device replacement is unsupported, including for a GPU-only
    /// static image with no source from which to reconstruct its contents.
    fn attach_device(
        &mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
    ) -> Result<(), String> {
        Err("Display does not support GPU device replacement".into())
    }

    /// Waits for an update and synchronizes inputs without host presentation.
    /// `Some(true)` requests a redraw; `Some(false)` only synchronizes resources.
    /// Return `None` when the source closes; its last image remains usable.
    /// Closure is terminal, including across host device replacement. Restore
    /// retained data in `attach_device` without expecting another update call.
    /// The host submits queued writes even when no redraw follows.
    ///
    /// This future must be cancellation safe: redraws can interrupt the wait.
    /// Consume bounded work per call and await when no update is available.
    /// The display and its futures need not be `Send`.
    fn update(&mut self, _queue: &wgpu::Queue) -> impl Future<Output = Option<bool>> {
        ready(None)
    }

    /// Records GPU work needed for the current display image, or reuses an
    /// existing image. Later commands in this encoder can draw the returned image.
    /// This records commands without submitting or waiting for GPU completion.
    /// Returns `None` when content is unavailable, including during guest recovery.
    /// Image storage can be reused; retaining an image does not preserve its pixels.
    ///
    /// The host must submit this encoder before the next `update` or `encode_image`.
    /// Implementations may cache the recorded result; discarding the encoder does
    /// not undo that cache and can leave stale pixels on subsequent calls.
    ///
    /// The host calls this on demand. Guest execution must progress
    /// independently of these calls.
    fn encode_image(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<&Image>;
}

/// An uploaded or GPU-generated image can also be connected as static content.
impl DisplayRenderer for Image {
    fn encode_image(&mut self, _encoder: &mut wgpu::CommandEncoder) -> Option<&Image> {
        Some(self)
    }
}

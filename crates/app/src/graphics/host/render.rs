use super::worker::Failure;
use crate::graphics::Surface;
use relocade_peripherals::display::DisplayRenderer;
use relocade_renderer::{fit_image, Rect, Renderer};
use winit::dpi::PhysicalSize;

pub(super) enum DrawOutcome {
    PresentRequested,
    Skipped,
    Retry,
}

/// Draws one requested frame. The worker decides when to draw again or recover.
pub(super) fn draw<D: DisplayRenderer>(
    surface: &mut Surface,
    display: &mut D,
    renderer: &mut Renderer,
    size: PhysicalSize<u32>,
) -> Result<DrawOutcome, Failure> {
    if size.width == 0 || size.height == 0 {
        return Ok(DrawOutcome::Skipped);
    }
    let frame = match surface.acquire(size).map_err(Failure::Fatal)? {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
        wgpu::CurrentSurfaceTexture::Occluded => return Ok(DrawOutcome::Skipped),
        // Lost can precede the asynchronous device-loss callback. Report it to
        // the worker so both paths use the same recovery policy.
        wgpu::CurrentSurfaceTexture::Lost => return Err(Failure::SurfaceLost),
        wgpu::CurrentSurfaceTexture::Validation => {
            return Err(Failure::Fatal(
                "Surface acquisition failed validation".into(),
            ));
        }
        _ => return Ok(DrawOutcome::Retry),
    };
    let mut encoder = surface.device.create_command_encoder(&Default::default());
    let image = display.encode_image(&mut encoder);
    let view = frame.texture.create_view(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        if let Some(image) = image {
            renderer.draw_image(
                &mut pass,
                image,
                fit_image(image, Rect::from_size(size.into())),
            );
        }
    }
    surface.queue.submit([encoder.finish()]);
    surface.present(frame);
    Ok(DrawOutcome::PresentRequested)
}

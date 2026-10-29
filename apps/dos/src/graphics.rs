use super::application::Event;
use futures_channel::mpsc;
use relocade_app::{
    graphics::{Surface, SurfaceSetup},
    winit::{dpi::PhysicalSize, event_loop::EventLoopProxy},
};
use relocade_renderer::{fit_image, Rect, Renderer};
use relocade_vga::{FrameSource, VgaImage, VgaRenderer};
use std::{future::pending, sync::Arc};

pub(super) async fn run(
    setup: SurfaceSetup,
    source: FrameSource,
    mut requests: mpsc::Receiver<PhysicalSize<u32>>,
    events: EventLoopProxy<Event>,
) -> Result<(), String> {
    let mut surface = Surface::new(setup).await?;
    let errors = events.clone();
    surface.device.on_uncaptured_error(Arc::new(move |error| {
        let _ = errors.send_event(Event::Finished(Err(format!("GPU: {error}"))));
    }));
    let errors = events.clone();
    surface
        .device
        .set_device_lost_callback(move |reason, message| {
            if reason != wgpu::DeviceLostReason::Destroyed {
                let _ =
                    errors.send_event(Event::Finished(Err(format!("GPU device lost: {message}"))));
            }
        });
    let mut vga = VgaRenderer::new(&surface.device, &surface.queue).map_err(|e| e.to_string())?;
    let mut renderer =
        Renderer::new(surface.device.clone(), surface.format()).map_err(|e| e.to_string())?;
    let mut image: Option<VgaImage> = None;
    let captures = async {
        let mut sequence = None;
        while let Some(capture) = source.next_capture(sequence).await {
            sequence = Some(capture.sequence);
            let _ = events.send_event(Event::Redraw);
        }
        // A closed source still leaves its last image available for host redraws.
        pending::<Result<(), String>>().await
    };
    let draw = async {
        let _ = events.send_event(Event::Redraw);
        while let Ok(size) = requests.recv().await {
            let frame = match surface.acquire(size)? {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                status => {
                    let occluded = matches!(status, wgpu::CurrentSurfaceTexture::Occluded);
                    let _ = events.send_event(Event::DrawComplete { occluded });
                    if !occluded {
                        let _ = events.send_event(Event::Redraw);
                    }
                    continue;
                }
            };
            let mut encoder = surface.device.create_command_encoder(&Default::default());
            if let Some(capture) = source.current_capture() {
                if image
                    .as_ref()
                    .is_none_or(|image| image.sequence != capture.sequence)
                {
                    image = Some(vga.encode(&surface.queue, &mut encoder, &capture));
                }
            }
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
                if let Some(image) = &image {
                    renderer.draw_image(
                        &mut pass,
                        &image.image,
                        fit_image(&image.image, Rect::from_size(size.into())),
                    );
                }
            }
            surface.queue.submit([encoder.finish()]);
            surface.present(frame);
            let _ = events.send_event(Event::DrawComplete { occluded: false });
        }
        Ok(())
    };
    futures_lite::future::or(captures, draw).await
}

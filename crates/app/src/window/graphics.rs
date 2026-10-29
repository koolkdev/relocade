use super::{
    host::Event, platform::SurfaceSetup, redraw::Requests, surface::WindowSurface,
    WindowApplication,
};
use arc_swap::ArcSwapOption;
use std::sync::Arc;
use winit::event_loop::EventLoopProxy;

pub(crate) async fn run<A: WindowApplication>(
    setup: SurfaceSetup,
    source: Arc<ArcSwapOption<A::Source>>,
    requests: Requests,
    events: EventLoopProxy<Event>,
) -> Result<(), String> {
    let mut surface = WindowSurface::new(setup, &events).await?;
    let mut graphics = A::create_graphics(
        surface.device.clone(),
        surface.queue.clone(),
        surface.format(),
    )?;
    let _ = events.send_event(Event::GraphicsReady);
    while let Some(size) = requests.next().await {
        if let Some(source) = source.load_full() {
            match surface.acquire(size, &events).await? {
                wgpu::CurrentSurfaceTexture::Success(frame)
                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    A::draw(
                        &mut graphics,
                        &source,
                        &frame.texture.create_view(&Default::default()),
                        size,
                    )?;
                    surface.present(frame);
                }
                // Wait for a host expose/resume instead of repeatedly acquiring
                // an occluded surface. Execution and its timers remain active.
                wgpu::CurrentSurfaceTexture::Occluded => continue,
                _ => {}
            }
        }
        #[cfg(target_arch = "wasm32")]
        // A ready Rust channel can keep a poll running indefinitely. Give canvas
        // presentation and browser GPU callbacks an event-loop task between frames.
        futures_timer::Delay::new(std::time::Duration::ZERO).await;
        let _ = events.send_event(Event::Presented);
    }
    Ok(())
}

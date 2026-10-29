use super::{host::Event, platform::SurfaceSetup, redraw::Requests, WindowApplication};
use arc_swap::ArcSwapOption;
use std::sync::Arc;
use winit::event_loop::EventLoopProxy;

pub(crate) async fn run<A: WindowApplication>(
    setup: SurfaceSetup,
    source: Arc<ArcSwapOption<A::Source>>,
    requests: Requests,
    events: EventLoopProxy<Event>,
) -> Result<(), String> {
    let (instance, surface, _window) = setup.into_surface()?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let (device, queue) = adapter
        .request_device(&Default::default())
        .await
        .map_err(|e| e.to_string())?;
    let mut config = surface
        .get_default_config(&adapter, 1, 1)
        .ok_or("Unsupported window surface")?;
    config.format = [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ]
    .into_iter()
    .find(|format| surface.get_capabilities(&adapter).formats.contains(format))
    .ok_or("Surface has no supported RGBA8/BGRA8 format")?;
    config.present_mode = wgpu::PresentMode::Fifo;
    let mut graphics = A::create_graphics(device.clone(), queue.clone(), config.format)?;
    let mut configured = false;
    let _ = events.send_event(Event::GraphicsReady);
    while let Some(size) = requests.next().await {
        if !size.contains(&0) {
            if size
                .iter()
                .any(|&n| n > device.limits().max_texture_dimension_2d)
            {
                return Err("Window exceeds the GPU texture size limit".into());
            }
            if !configured || size != [config.width, config.height] {
                config.width = size[0];
                config.height = size[1];
                surface.configure(&device, &config);
                configured = true;
            }
            if let Some(source) = source.load_full() {
                match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                        A::draw(
                            &mut graphics,
                            &source,
                            &frame.texture.create_view(&Default::default()),
                            size,
                        )?;
                        #[cfg(not(target_arch = "wasm32"))]
                        _window.pre_present_notify();
                        queue.present(frame);
                    }
                    wgpu::CurrentSurfaceTexture::Timeout
                    | wgpu::CurrentSurfaceTexture::Occluded => {}
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        configured = false
                    }
                    wgpu::CurrentSurfaceTexture::Validation => {
                        return Err("Surface acquisition failed validation".into())
                    }
                }
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

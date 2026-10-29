use super::{
    host::Event,
    platform::{SurfaceSetup, SurfaceTarget},
};
use std::sync::Arc;
use winit::event_loop::EventLoopProxy;

/// The graphics worker's surface configuration, acquisition and presentation owner.
pub(crate) struct WindowSurface {
    surface: Option<wgpu::Surface<'static>>,
    target: SurfaceTarget,
    adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    configured: bool,
}
impl WindowSurface {
    pub async fn new(setup: SurfaceSetup, events: &EventLoopProxy<Event>) -> Result<Self, String> {
        let (target, surface) = setup.into_surface()?;
        let adapter = target
            .instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|error| error.to_string())?;
        let (device, queue) = adapter
            .request_device(&Default::default())
            .await
            .map_err(|error| error.to_string())?;
        let lost = events.clone();
        device.set_device_lost_callback(move |reason, message| {
            let _ = lost.send_event(Event::Failed(format!(
                "GPU device lost ({reason:?}): {message}"
            )));
        });
        let errors = events.clone();
        device.on_uncaptured_error(Arc::new(move |error| {
            let _ = errors.send_event(Event::Failed(format!("GPU error: {error}")));
        }));
        let mut config = surface
            .get_default_config(&adapter, 1, 1)
            .ok_or("Unsupported window surface")?;
        let formats = surface.get_capabilities(&adapter).formats;
        config.format = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|format| formats.contains(format))
        .ok_or("Surface has no supported RGBA8/BGRA8 format")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        Ok(Self {
            surface: Some(surface),
            target,
            adapter,
            device,
            queue,
            config,
            configured: false,
        })
    }
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub async fn acquire(
        &mut self,
        size: [u32; 2],
        events: &EventLoopProxy<Event>,
    ) -> Result<wgpu::CurrentSurfaceTexture, String> {
        if size
            .iter()
            .any(|&n| n == 0 || n > self.device.limits().max_texture_dimension_2d)
        {
            return Err("Window exceeds the GPU texture size limits".into());
        }
        let surface = self
            .surface
            .as_ref()
            .expect("surface exists outside recovery");
        if !self.configured || size != [self.config.width, self.config.height] {
            self.config.width = size[0];
            self.config.height = size[1];
            surface.configure(&self.device, &self.config);
            self.configured = true;
        }
        let frame = surface.get_current_texture();
        match &frame {
            wgpu::CurrentSurfaceTexture::Suboptimal(_) | wgpu::CurrentSurfaceTexture::Outdated => {
                self.configured = false
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.recreate(events).await?;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("Surface acquisition failed validation".into())
            }
            _ => {}
        }
        Ok(frame)
    }
    async fn recreate(&mut self, events: &EventLoopProxy<Event>) -> Result<(), String> {
        // Release the old surface before recreating it on the platform owner.
        self.surface = None;
        let surface = self.target.recreate(events).await?;
        let capabilities = surface.get_capabilities(&self.adapter);
        // The default Auto alpha mode is resolved at configure time and need
        // not appear in capabilities. Only our retained image format is fixed.
        if !capabilities.formats.contains(&self.config.format) {
            return Err("Recovered surface is incompatible with the current GPU resources".into());
        }
        self.surface = Some(surface);
        self.configured = false;
        Ok(())
    }
    pub fn present(&self, frame: wgpu::SurfaceTexture) {
        self.target.before_present();
        self.queue.present(frame);
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "surface/tests.rs"]
mod tests;

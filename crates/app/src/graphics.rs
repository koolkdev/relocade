//! Surface initialization, acquisition and presentation on the graphics owner.
//! Transfer `Window::surface_setup` using the existing `worker::Thread`, then
//! create `Surface` there. Command recording and image drawing remain with the caller.
use crate::platform::graphics::SurfaceOwner;
pub use crate::platform::graphics::SurfaceSetup;
use std::{marker::PhantomData, rc::Rc};
use winit::dpi::PhysicalSize;

/// Owns GPU resources and surface operations on the thread that initializes it.
pub struct Surface {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    owner: SurfaceOwner,
    config: wgpu::SurfaceConfiguration,
    configured: bool,
    _owner_thread: PhantomData<Rc<()>>,
}

impl Surface {
    pub async fn new(setup: SurfaceSetup) -> Result<Self, String> {
        let owner = setup.open()?;
        let (device, queue, config) = Self::request_device(&owner).await?;
        Ok(Self {
            device,
            queue,
            owner,
            config,
            configured: false,
            _owner_thread: PhantomData,
        })
    }

    /// Replaces the device and queue while retaining the platform surface target.
    /// Drop attachments and all resources from the old device before calling.
    /// Rebuild images and pipelines afterward, using the new device and format.
    /// This does not repair a lost platform surface or transfer a canvas again.
    pub async fn recreate_device(&mut self) -> Result<(), String> {
        let (device, queue, config) = Self::request_device(&self.owner).await?;
        self.device = device;
        self.queue = queue;
        self.config = config;
        self.configured = false;
        Ok(())
    }

    async fn request_device(
        owner: &SurfaceOwner,
    ) -> Result<(wgpu::Device, wgpu::Queue, wgpu::SurfaceConfiguration), String> {
        let adapter = owner
            .instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&owner.surface),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let (device, queue) = adapter
            .request_device(&Default::default())
            .await
            .map_err(|e| e.to_string())?;
        let mut config = owner
            .surface
            .get_default_config(&adapter, 1, 1)
            .ok_or("Unsupported window surface")?;
        let formats = owner.surface.get_capabilities(&adapter).formats;
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
        Ok((device, queue, config))
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Configures the current physical size and acquires one output attachment.
    /// Call only for nonzero output. Wgpu's acquisition status is preserved so the
    /// caller can distinguish retrying a timeout from waiting for an occluded window.
    /// Outdated configurations are refreshed on the next requested draw.
    /// Present or drop the previous frame before calling again: a resize or
    /// outdated configuration cannot be applied while that frame is alive.
    pub fn acquire(
        &mut self,
        size: PhysicalSize<u32>,
    ) -> Result<wgpu::CurrentSurfaceTexture, String> {
        if size.width == 0
            || size.height == 0
            || size.width > self.device.limits().max_texture_dimension_2d
            || size.height > self.device.limits().max_texture_dimension_2d
        {
            return Err("Window dimensions must be nonzero and within GPU texture limits".into());
        }
        if !self.configured || [size.width, size.height] != [self.config.width, self.config.height]
        {
            self.config.width = size.width;
            self.config.height = size.height;
            self.owner.surface.configure(&self.device, &self.config);
            self.configured = true;
        }
        let frame = self.owner.surface.get_current_texture();
        match &frame {
            wgpu::CurrentSurfaceTexture::Suboptimal(_) | wgpu::CurrentSurfaceTexture::Outdated => {
                self.configured = false;
            }
            _ => {}
        }
        Ok(frame)
    }

    /// Call after submitting the commands that draw into this frame.
    pub fn present(&self, frame: wgpu::SurfaceTexture) {
        self.owner.present(&self.queue, frame);
    }
}

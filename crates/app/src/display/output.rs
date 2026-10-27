use crate::platform::RenderTarget;

/// The host surface and its frame lifecycle, independent of application drawing.
pub(super) struct Output {
    target: RenderTarget,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    configured: bool,
}

impl Output {
    pub fn new(
        target: RenderTarget,
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        surface: wgpu::Surface<'static>,
    ) -> Result<Self, String> {
        let mut config = surface
            .get_default_config(&adapter, 1, 1)
            .ok_or("The graphics adapter cannot present to this window")?;
        let formats = surface.get_capabilities(&adapter).formats;
        config.format = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ]
        .into_iter()
        .find(|format| formats.contains(format))
        .unwrap_or(config.format);
        config.present_mode = wgpu::PresentMode::Fifo;
        Ok(Self {
            target,
            instance,
            adapter,
            surface: Some(surface),
            config,
            configured: false,
        })
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn suspend(&mut self) {
        self.surface = None;
        self.configured = false;
    }

    pub async fn resume(&mut self) -> Result<(), String> {
        if self.surface.is_none() {
            let surface = self.target.surface(&self.instance).await?;
            if !surface
                .get_capabilities(&self.adapter)
                .formats
                .contains(&self.config.format)
            {
                return Err(
                    "The recreated surface no longer supports the host output format".into(),
                );
            }
            let mut config = surface
                .get_default_config(&self.adapter, 1, 1)
                .ok_or("The graphics adapter cannot present to this window")?;
            config.format = self.config.format;
            config.present_mode = wgpu::PresentMode::Fifo;
            self.config = config;
            self.surface = Some(surface);
            self.configured = false;
        }
        Ok(())
    }

    pub async fn acquire(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
    ) -> Result<Option<wgpu::SurfaceTexture>, String> {
        if size.contains(&0) {
            return Ok(None);
        }
        if size
            .iter()
            .any(|&n| n > device.limits().max_texture_dimension_2d)
        {
            return Err("The window exceeds the graphics device's maximum texture size".into());
        }
        self.resume().await?;
        let surface = self.surface.as_ref().expect("resumed output surface");
        if !self.configured || size != [self.config.width, self.config.height] {
            self.config.width = size[0];
            self.config.height = size[1];
            surface.configure(device, &self.config);
            self.configured = true;
        }
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Ok(Some(frame)),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.configured = false;
                Ok(Some(frame))
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configured = false;
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.suspend();
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                Err("Surface acquisition failed validation".into())
            }
        }
    }

    pub fn present(&self, queue: &wgpu::Queue, frame: wgpu::SurfaceTexture) {
        self.target.pre_present_notify();
        queue.present(frame);
    }
}

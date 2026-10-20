mod error;
mod texture;

pub use error::FrameError;
use texture::FrameTexture;

/// Draws tightly packed RGBA8 frames with nearest-neighbor scaling.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    source_format: wgpu::TextureFormat,
    texture: Option<FrameTexture>,
}

impl Renderer {
    /// Uses the given device and its queue. Output views must be single-sampled
    /// RGBA8 or BGRA8, with the same `format` and device as this renderer.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<Self, FrameError> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(FrameError::UnsupportedFormat(format));
        }
        // Source bytes are sRGB. Decode only when the output attachment will
        // encode linear shader results back to sRGB.
        let source_format = if format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/frame.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pixel frame"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest pixel"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            sampler,
            source_format,
            texture: None,
        })
    }

    /// Uploads and fills `target` with nearest-neighbor sampling and no blending.
    /// Pixels are top-to-bottom, tightly packed RGBA8 with sRGB color values.
    /// The source texture is reused until `size` changes; the target can have a
    /// different size on every call. The embedding chooses its display aspect
    /// ratio through the target dimensions and skips drawing to zero-sized surfaces.
    ///
    /// `target` must allow `RENDER_ATTACHMENT` usage. For a window or canvas,
    /// acquire a surface texture, draw into its view, then present that texture.
    /// This method submits work without waiting for the GPU.
    pub fn render(
        &mut self,
        size: [u32; 2],
        pixels: &[u8],
        target: &wgpu::TextureView,
    ) -> Result<(), FrameError> {
        validate(
            size,
            pixels.len(),
            self.device.limits().max_texture_dimension_2d,
        )?;
        if self
            .texture
            .as_ref()
            .is_none_or(|texture| texture.size != size)
        {
            self.texture = Some(FrameTexture::new(
                &self.device,
                &self.pipeline.get_bind_group_layout(0),
                &self.sampler,
                size,
                self.source_format,
            ));
        }
        let texture = self.texture.as_ref().expect("frame texture");
        texture.upload(&self.queue, pixels);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pixel frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &texture.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
}

fn validate(size: [u32; 2], len: usize, max_dimension: u32) -> Result<(), FrameError> {
    if size.iter().any(|&n| n == 0 || n > max_dimension) {
        return Err(FrameError::InvalidSize);
    }
    let expected = (size[0] as usize)
        .checked_mul(size[1] as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(FrameError::InvalidSize)?;
    if len != expected {
        return Err(FrameError::InvalidLength {
            expected,
            actual: len,
        });
    }
    Ok(())
}

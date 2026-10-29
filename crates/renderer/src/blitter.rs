use crate::{FrameError, Image};

/// Nearest-neighbor viewing with centered, aspect-preserving placement.
/// Owns a draw pipeline and bindings, but no surface, worker or render schedule.
pub struct ImageBlitter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    binding: Option<(wgpu::Texture, wgpu::BindGroup)>,
}

impl ImageBlitter {
    /// Output attachments must use this device and format, with one sample.
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
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/frame.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image blitter"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if format.is_srgb() {
                    "fragment_srgb"
                } else {
                    "fragment"
                }),
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
            binding: None,
        })
    }

    /// Fits the image into `size`, clearing unused borders to opaque black.
    /// `size` must be the extent of the supplied attachment's mip level. The
    /// attachment must allow `RENDER_ATTACHMENT` and use this blitter's format.
    /// Submits work without waiting, changing the source image, or presenting.
    pub fn blit(
        &mut self,
        image: &Image,
        target: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), FrameError> {
        if size
            .iter()
            .any(|&n| n == 0 || n > self.device.limits().max_texture_dimension_2d)
        {
            return Err(FrameError::InvalidSize);
        }
        if self
            .binding
            .as_ref()
            .is_none_or(|(bound, _)| bound != image.texture())
        {
            let binding = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("viewed image"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(image.view()),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.binding = Some((image.texture().clone(), binding));
        }
        let (_, binding) = self.binding.as_ref().expect("bound image");
        let [x, y, width, height] = placement(image.size(), image.pixel_aspect_ratio(), size);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("image blitter"),
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
            pass.set_bind_group(0, binding, &[]);
            pass.set_viewport(x, y, width, height, 0.0, 1.0);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
}

fn placement(image: [u32; 2], aspect: (u32, u32), output: [u32; 2]) -> [f32; 4] {
    let ratio =
        f64::from(image[0]) * f64::from(aspect.0) / (f64::from(image[1]) * f64::from(aspect.1));
    let width = f64::from(output[0]).min(f64::from(output[1]) * ratio);
    let height = f64::from(output[1]).min(f64::from(output[0]) / ratio);
    [
        ((f64::from(output[0]) - width) / 2.0) as f32,
        ((f64::from(output[1]) - height) / 2.0) as f32,
        width as f32,
        height as f32,
    ]
}

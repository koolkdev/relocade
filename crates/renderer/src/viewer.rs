use crate::{Error, Texture};

/// Scales a texture into a host attachment with nearest-neighbor sampling.
pub struct Viewer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    srgb_output: bool,
    binding: Option<(Texture, wgpu::BindGroup)>,
}

impl Viewer {
    /// Uses the given device and its queue. Output views must be single-sampled
    /// RGBA8 or BGRA8, with the same `format` and device as this viewer.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Error> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(Error::UnsupportedFormat(format));
        }
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/frame.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("host texture viewer"),
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
            srgb_output: format.is_srgb(),
            binding: None,
        })
    }

    /// Fills the host attachment with the current texture, without changing its
    /// pixels or dimensions. The target's device and format must match this viewer.
    /// This submits host drawing without waiting for the GPU or presenting it.
    pub fn draw(&mut self, texture: &Texture, target: &wgpu::TextureView) {
        if self
            .binding
            .as_ref()
            .is_none_or(|(bound, _)| !bound.same_texture(texture))
        {
            let binding = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("host texture"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            texture.sampling_view(self.srgb_output),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.binding = Some((texture.clone(), binding));
        }
        let (_, binding) = self.binding.as_ref().expect("bound texture");
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("host texture viewer"),
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
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }
}

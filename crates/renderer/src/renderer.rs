use crate::{FrameError, Image, Rect};

/// Shared drawing operations recorded into caller-owned render passes.
/// Currently draws whole RGBA8 images with nearest sampling and no blending.
pub struct Renderer {
    device: wgpu::Device,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    binding: Option<(wgpu::Texture, wgpu::BindGroup)>,
}

impl Renderer {
    /// Output attachments must use this device and format, with one sample.
    pub fn new(device: wgpu::Device, format: wgpu::TextureFormat) -> Result<Self, FrameError> {
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
            label: Some("image drawing"),
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
            pipeline,
            sampler,
            binding: None,
        })
    }

    /// Draws the entire image into `destination`, in attachment pixel coordinates.
    /// Records pipeline, binding 0 and viewport state; retains the caller's scissor.
    /// The destination must fit inside the attachment; empty rectangles draw nothing.
    /// The pass must use this renderer's device, output format and one sample.
    /// Clearing, pass lifetime, and submission belong to the caller.
    pub fn draw_image(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        image: &Image,
        destination: Rect,
    ) {
        if destination.width == 0.0 || destination.height == 0.0 {
            return;
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
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, binding, &[]);
        pass.set_viewport(
            destination.x,
            destination.y,
            destination.width,
            destination.height,
            0.0,
            1.0,
        );
        pass.draw(0..3, 0..1);
    }
}

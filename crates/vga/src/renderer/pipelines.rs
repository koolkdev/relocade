//! Cached decoders with one explicit layout for their shared input resources.
use super::shaders;
use crate::Scanout;
use relocade_renderer::Image;

pub(super) struct DecodePipelines {
    vga: wgpu::RenderPipeline,
    svga: wgpu::RenderPipeline,
}

impl DecodePipelines {
    pub fn new(device: &wgpu::Device, inputs: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("display decoding"),
            bind_group_layouts: &[Some(inputs)],
            ..Default::default()
        });
        let create = |label, source: &'static str| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
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
                    targets: &[Some(Image::FORMAT.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            vga: create("VGA decoding", shaders::VGA),
            svga: create("packed SVGA decoding", shaders::SVGA),
        }
    }

    pub fn for_scanout(&self, scanout: Scanout) -> &wgpu::RenderPipeline {
        match scanout {
            Scanout::Vga(_) => &self.vga,
            Scanout::Svga(_) => &self.svga,
        }
    }
}

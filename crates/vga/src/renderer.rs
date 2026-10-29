mod memory;
mod text_input;
mod upload;

use crate::CapturedFrame;
use relocade_renderer::{FrameError, Image};
use std::time::Duration;
use text_input::TextInput;

/// Reusable GPU output with metadata for the capture recorded by `encode`.
/// The caller must submit the recorded work before consuming the updated pixels.
/// Encodes with the same geometry reuse storage; retaining that image does not
/// retain pixel history. A geometry change allocates a new image.
pub struct VgaImage {
    pub image: Image,
    pub time: Duration,
    pub sequence: u64,
}

/// GPU text decoding recorded into a caller-owned command encoder.
/// Owns captured-input resources and one reusable output image. Accesses no live
/// VGA state and owns no host output pass, surface, worker, or submission schedule.
pub struct VgaRenderer {
    device: wgpu::Device,
    pipeline: wgpu::RenderPipeline,
    input: TextInput,
    image: Image,
}

impl VgaRenderer {
    /// Requires a 2D texture limit of at least 2304 pixels, covering every VGA
    /// text width. Uses integer sampled textures and uniform buffers.
    pub fn new(device: &wgpu::Device) -> Result<Self, FrameError> {
        // The CRTC can select up to 256 nine-dot columns and 1024 lines.
        if device.limits().max_texture_dimension_2d < 256 * 9 {
            return Err(FrameError::InvalidSize);
        }
        let image = Image::new(device, [640, 200], (5, 12))?;
        let shader = device.create_shader_module(wgpu::include_wgsl!("renderer/text.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("VGA text decoding"),
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
                targets: &[Some(Image::FORMAT.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let input = TextInput::new(device, &pipeline.get_bind_group_layout(0));
        Ok(Self {
            device: device.clone(),
            pipeline,
            input,
            image,
        })
    }

    /// Queues changed required input words and records decoding into its image.
    /// Always records a decode, including when retrying a discarded encoder.
    /// The returned metadata describes recorded work, not completed GPU output.
    ///
    /// Select one capture version per renderer per submitted batch. Buffer and texture
    /// uploads run before that batch, so several captures cannot share this input
    /// storage in one submission. Submit or discard an update before encoding another.
    /// Retain the returned image for resize-only redraws without another encode.
    pub fn encode(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        capture: &CapturedFrame,
    ) -> VgaImage {
        let text = capture.display.text_display();
        let aspect = text.layout.pixel_aspect_ratio();
        if self.image.size() != text.layout.size || self.image.pixel_aspect_ratio() != aspect {
            self.image = Image::new(&self.device, text.layout.size, aspect)
                .expect("VGA text dimensions fit the validated device limits");
        }
        self.input.upload(queue, &capture.display, &text);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("VGA text decoding"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.image.view(),
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
            pass.set_bind_group(0, &self.input.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        VgaImage {
            image: self.image.clone(),
            time: capture.time,
            sequence: capture.sequence,
        }
    }
}

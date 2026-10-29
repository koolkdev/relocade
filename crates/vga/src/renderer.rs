mod input;

use crate::{text, CapturedFrame};
use input::TextInput;
use relocade_renderer::{FrameError, Image};
use std::time::Duration;

/// Reusable GPU output with metadata for the capture recorded by `encode`.
/// The caller must submit the recorded work before consuming the updated pixels.
/// Later encodes reuse this storage; retaining the image does not retain history.
pub struct VgaImage {
    pub image: Image,
    pub time: Duration,
    pub sequence: u64,
}

/// GPU text decoding recorded into a caller-owned command encoder.
/// Owns captured-input resources and one reusable output image. Accesses no live
/// VGA state and owns no host output pass, surface, worker, or submission schedule.
pub struct VgaRenderer {
    pipeline: wgpu::RenderPipeline,
    input: TextInput,
    image: Image,
}

impl VgaRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self, FrameError> {
        let image = Image::new(
            device,
            [text::WIDTH, text::HEIGHT],
            text::PIXEL_ASPECT_RATIO,
        )?;
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
        let input = TextInput::new(device, queue, &pipeline.get_bind_group_layout(0));
        Ok(Self {
            pipeline,
            input,
            image,
        })
    }

    /// Queues captured text input and records the pass that overwrites its private image.
    /// Always records a decode, including when retrying a discarded encoder.
    /// The returned metadata describes recorded work, not completed GPU output.
    ///
    /// Select one capture version per renderer per submitted batch. `write_buffer`
    /// uploads run before that batch, so several captures cannot share this input
    /// storage in one submission. Submit or discard an update before encoding another.
    /// Retain the returned image for resize-only redraws without another encode.
    pub fn encode(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        capture: &CapturedFrame,
    ) -> VgaImage {
        self.input.upload(queue, capture);
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

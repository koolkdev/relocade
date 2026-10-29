mod input;

use crate::{text, CapturedFrame};
use input::TextInput;
use relocade_renderer::{FrameError, Image};
use std::{sync::Arc, time::Duration};

/// The renderer's current GPU image with source capture metadata.
/// Dimensions and pixel aspect ratio are carried by `image`. Storage is reused
/// on later renders; retaining an image clone does not retain historical pixels.
pub struct VgaImage {
    /// Renderer-owned storage for sampling or copying. Leave its contents
    /// intact so rendering the same capture can reuse them.
    pub image: Image,
    pub time: Duration,
    pub sequence: u64,
}

/// Explicit GPU rendering of immutable fixed-mode text captures.
/// Owns no device state, surface, worker, timer, or redraw loop. The caller
/// orders rendering and viewing on the supplied device and queue.
pub struct VgaRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    input: TextInput,
    frame: VgaImage,
    capture: Option<Arc<CapturedFrame>>,
}

impl VgaRenderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Result<Self, FrameError> {
        let image = Image::new(
            &device,
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
        let input = TextInput::new(&device, &queue, &pipeline.get_bind_group_layout(0));
        Ok(Self {
            device,
            queue,
            pipeline,
            input,
            frame: VgaImage {
                image,
                time: Duration::ZERO,
                sequence: 0,
            },
            capture: None,
        })
    }

    /// Uploads text state and submits decoding only when given a different capture.
    /// Capture identity also distinguishes independent sources with equal sequences.
    /// Repeated calls for the same capture reuse the image. No CPU pixel conversion
    /// or GPU readback occurs; completion is ordered before later queue submissions.
    pub fn render(&mut self, capture: &Arc<CapturedFrame>) -> &VgaImage {
        if self
            .capture
            .as_ref()
            .is_none_or(|previous| !Arc::ptr_eq(previous, capture))
        {
            self.input.upload(&self.queue, capture);
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("VGA text decoding"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: self.frame.image.view(),
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
            self.queue.submit([encoder.finish()]);
            self.frame.time = capture.time;
            self.frame.sequence = capture.sequence;
            self.capture = Some(capture.clone());
        }
        &self.frame
    }
}

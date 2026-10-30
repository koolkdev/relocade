mod memory;
mod shaders;
mod state;

use crate::CapturedFrame;
use memory::GpuMemory;
use relocade_renderer::{FrameError, Image};
use state::GpuState;
use std::time::Duration;

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
    memory: GpuMemory,
    state: GpuState,
    image: Image,
    frame: Option<FrameMetadata>,
}

struct FrameMetadata {
    time: Duration,
    sequence: u64,
}

impl VgaRenderer {
    /// Requires a 2D texture limit of at least 2304 pixels, covering every VGA
    /// text width, 11 flat inter-stage variables, and one 256-KiB fragment storage
    /// buffer for physical VRAM.
    pub fn new(device: &wgpu::Device) -> Result<Self, FrameError> {
        // The CRTC can select up to 256 nine-dot columns and 1024 lines.
        let limits = device.limits();
        if limits.max_texture_dimension_2d < 256 * 9
            || limits.max_inter_stage_shader_variables < 11
            || limits.max_storage_buffers_per_shader_stage < 1
            || limits.max_storage_buffer_binding_size < memory::BUFFER_BYTES
            || limits.max_buffer_size < memory::BUFFER_BYTES
        {
            return Err(FrameError::InvalidSize);
        }
        let image = Image::new(device, [640, 200], (5, 12))?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("VGA text decoding"),
            source: wgpu::ShaderSource::Wgsl(shaders::TEXT.into()),
        });
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
        let memory = GpuMemory::new(device);
        let state = GpuState::new(device, &pipeline.get_bind_group_layout(0), &memory.buffer);
        Ok(Self {
            device: device.clone(),
            pipeline,
            memory,
            state,
            image,
            frame: None,
        })
    }

    /// Uploads complete captured inputs into persistent GPU resources without
    /// decoding or presenting an image. Captures can be uploaded independently
    /// of redraws. Call `queue.submit([])` when no drawing follows.
    /// Complete retained captures can also restore older input state.
    pub fn upload(&mut self, queue: &wgpu::Queue, capture: &CapturedFrame) {
        let geometry = capture.display.geometry();
        let aspect = geometry.pixel_aspect_ratio();
        if self.image.size() != geometry.size || self.image.pixel_aspect_ratio() != aspect {
            self.image = Image::new(&self.device, geometry.size, aspect)
                .expect("VGA text dimensions fit the validated device limits");
        }
        self.memory.upload(queue, &capture.display);
        self.state.upload(queue, &capture.display, geometry);
        self.frame = Some(FrameMetadata {
            time: capture.time,
            sequence: capture.sequence,
        });
    }

    /// Records decoding of the latest uploaded inputs. Returns `None` before the
    /// first upload. Always records a draw, including retries of discarded work.
    ///
    /// Submit or discard this encoder before uploading another capture: queued
    /// writes run before submitted commands and the input buffers hold one version.
    /// Retain the returned image for resize-only draws without another decode.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<VgaImage> {
        let frame = self.frame.as_ref()?;
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
            pass.set_bind_group(0, &self.state.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Some(VgaImage {
            image: self.image.clone(),
            time: frame.time,
            sequence: frame.sequence,
        })
    }
}

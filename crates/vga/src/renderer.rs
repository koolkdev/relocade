mod memory;
mod pipelines;
mod shaders;
mod state;

use crate::{DisplaySnapshot, Scanout};
use memory::GpuMemory;
use pipelines::DecodePipelines;
use relocade_renderer::{FrameError, Image};
use state::GpuState;

/// GPU VGA and packed SVGA decoding recorded into a caller-owned command encoder.
/// Owns captured-input resources and one reusable output image. Accesses no live
/// VGA state and owns no host output pass, surface, worker, or submission schedule.
pub struct VgaRenderer {
    device: wgpu::Device,
    pipelines: DecodePipelines,
    memory: GpuMemory,
    state: GpuState,
    image: Image,
    scanout: Option<Scanout>,
}

impl VgaRenderer {
    /// Requires a 2D texture limit of at least 2304 pixels, covering every VGA
    /// text width, four flat inter-stage variables for VGA, two uniform buffers,
    /// and a 4-MiB fragment storage limit. Memory grows with captured blocks.
    pub fn new(device: &wgpu::Device) -> Result<Self, FrameError> {
        // The CRTC can select 256 nine-dot columns and 2048 physical scanlines.
        let limits = device.limits();
        if limits.max_texture_dimension_2d < 256 * 9
            || limits.max_inter_stage_shader_variables < 4
            || limits.max_uniform_buffers_per_shader_stage < 2
            || limits.max_storage_buffers_per_shader_stage < 1
            || limits.max_storage_buffer_binding_size < memory::REQUIRED_BUFFER_LIMIT
            || limits.max_buffer_size < memory::REQUIRED_BUFFER_LIMIT
        {
            return Err(FrameError::InvalidSize);
        }
        let image = Image::new(device, [640, 200], (5, 12))?;
        let memory = GpuMemory::new(device);
        let state = GpuState::new(device, &memory.buffer);
        let pipelines = DecodePipelines::new(device, &state.layout);
        Ok(Self {
            device: device.clone(),
            pipelines,
            memory,
            state,
            image,
            scanout: None,
        })
    }

    /// Synchronizes captured inputs into persistent GPU resources without
    /// decoding or presenting an image. Uploads changed pages when their baseline
    /// matches, otherwise complete blocks. Call `queue.submit([])` when no drawing
    /// follows. Captures can be uploaded independently of redraws.
    /// Complete retained captures can also restore older input state.
    /// Returns whether rendering inputs or output geometry changed.
    pub fn upload(&mut self, queue: &wgpu::Queue, snapshot: &DisplaySnapshot) -> bool {
        let scanout = snapshot.scanout();
        let scanout_changed = self.scanout.as_ref() != Some(scanout);
        let geometry = snapshot.geometry();
        let aspect = geometry.pixel_aspect_ratio();
        let geometry_changed =
            self.image.size() != geometry.size || self.image.pixel_aspect_ratio() != aspect;
        if geometry_changed {
            self.image = Image::new(&self.device, geometry.size, aspect)
                .expect("Captured dimensions fit the validated device limits");
        }
        if self.memory.ensure_size(&self.device, snapshot) {
            self.state.rebind_memory(&self.device, &self.memory.buffer);
        }
        let memory_changed = self.memory.upload(queue, snapshot);
        let state_changed = self.state.upload(queue, snapshot);
        self.scanout = Some(scanout.clone());
        scanout_changed || geometry_changed || memory_changed || state_changed
    }

    /// Records decoding of the latest uploaded inputs. Returns `None` before the
    /// first upload. Always records a draw, including retries of discarded work.
    ///
    /// Submit or discard this encoder before uploading another capture: queued
    /// writes run before submitted commands and the input buffers hold one version.
    /// Retain the returned image for resize-only draws without another decode.
    /// Encodes with the same geometry reuse image storage; retaining that image
    /// does not retain pixel history. A geometry change allocates a new image.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<Image> {
        let scanout = self.scanout.as_ref()?;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("display decoding"),
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
            pass.set_pipeline(self.pipelines.for_scanout(scanout));
            pass.set_bind_group(0, &self.state.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Some(self.image.clone())
    }
}

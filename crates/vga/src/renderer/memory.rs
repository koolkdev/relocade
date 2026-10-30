//! A complete GPU copy of physical VRAM, independent of display layout.
use crate::{memory::PLANE_BYTES, DisplaySnapshot};

pub(super) const BUFFER_BYTES: u64 = 4 * PLANE_BYTES as u64;

pub(super) struct GpuMemory {
    pub buffer: wgpu::Buffer,
}

impl GpuMemory {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("physical VGA planes"),
                size: BUFFER_BYTES,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    pub fn upload(&mut self, queue: &wgpu::Queue, snapshot: &DisplaySnapshot) {
        for (plane, bytes) in snapshot.planes.iter().enumerate() {
            queue.write_buffer(&self.buffer, (plane * PLANE_BYTES) as u64, bytes);
        }
    }
}

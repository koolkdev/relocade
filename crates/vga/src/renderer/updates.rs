//! Pending physical VRAM bounds; the shader owns their mapping to pixels.
use crate::memory::PLANE_BYTES;
use std::ops::Range;
use wgpu::util::DeviceExt;

pub(super) struct GpuUpdates {
    buffer: wgpu::Buffer,
    pending: Option<[Range<usize>; 4]>,
    pub binding: wgpu::BindGroup,
    pub full_binding: wgpu::BindGroup,
}

impl GpuUpdates {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let buffer = |words: [u32; 8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("VGA update bounds"),
                contents: words.map(u32::to_le_bytes).as_flattened(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        };
        let binding = |buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("VGA update bounds"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        let pending_buffer = buffer([0; 8]);
        let mut full_bounds = [0; 8];
        full_bounds[4..].fill(PLANE_BYTES as u32);
        let full_buffer = buffer(full_bounds);
        Self {
            binding: binding(&pending_buffer),
            full_binding: binding(&full_buffer),
            buffer: pending_buffer,
            pending: None,
        }
    }

    pub fn upload(&mut self, queue: &wgpu::Queue, bounds: [Range<usize>; 4], full: bool) {
        let pending = self.pending.get_or_insert_with(Default::default);
        for (pending, range) in pending.iter_mut().zip(bounds) {
            if full {
                *pending = 0..PLANE_BYTES;
            } else if !range.is_empty() {
                if pending.start == pending.end {
                    *pending = range;
                } else {
                    pending.start = pending.start.min(range.start);
                    pending.end = pending.end.max(range.end);
                }
            }
        }
        // WGSL uses two vec4<u32> fields: all starts, then all exclusive ends.
        let words: [u32; 8] = std::array::from_fn(|index| {
            let range = &pending[index % 4];
            if index < 4 {
                range.start as u32
            } else {
                range.end as u32
            }
        });
        queue.write_buffer(&self.buffer, 0, words.map(u32::to_le_bytes).as_flattened());
    }

    /// The display host submits the recorded image before another upload.
    pub fn take_pending(&mut self) -> bool {
        self.pending.take().is_some()
    }
}

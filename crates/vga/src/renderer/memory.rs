//! Persistent GPU storage for captured physical video memory.
use crate::{
    memory::{MemorySnapshot, BLOCK_BYTES, VGA_MEMORY_BYTES},
    DisplaySnapshot,
};
use std::sync::Arc;

// The renderer accepts every current card on devices meeting this resource limit.
// Allocation grows with captured memory rather than reserving that limit upfront.
pub(super) const REQUIRED_BUFFER_LIMIT: u64 = 4 * 1024 * 1024;

pub(super) struct GpuMemory {
    pub buffer: wgpu::Buffer,
    /// Immutable block identities describing the last queued upload.
    previous: Option<MemorySnapshot>,
}

impl GpuMemory {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("captured video memory"),
                size: u64::from(VGA_MEMORY_BYTES),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            previous: None,
        }
    }

    pub fn ensure_size(&mut self, device: &wgpu::Device, snapshot: &DisplaySnapshot) -> bool {
        let bytes = snapshot.memory.bytes() as u64;
        if bytes <= self.buffer.size() {
            return false;
        }
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("captured video memory"),
            size: bytes.next_power_of_two(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.previous = None;
        true
    }

    pub fn upload(&mut self, queue: &wgpu::Queue, snapshot: &DisplaySnapshot) -> bool {
        let mut changed = false;
        write_blocks(self.previous.as_ref(), snapshot, |offset, bytes| {
            queue.write_buffer(&self.buffer, offset as u64, bytes);
            changed = true;
        });
        self.previous = Some(snapshot.memory.clone());
        changed
    }
}

fn write_blocks(
    previous: Option<&MemorySnapshot>,
    snapshot: &DisplaySnapshot,
    mut write: impl FnMut(usize, &[u8]),
) {
    for (block, bytes) in snapshot.memory.blocks.iter().enumerate() {
        let previous = previous.and_then(|memory| memory.blocks.get(block));
        if previous.is_some_and(|previous| Arc::ptr_eq(previous, bytes)) {
            continue;
        }
        let changes = snapshot.changes.as_ref().filter(|changes| {
            previous
                .zip(changes.baseline.blocks.get(block))
                .is_some_and(|(previous, baseline)| Arc::ptr_eq(previous, baseline))
        });
        let base = block * BLOCK_BYTES;
        if let Some(changes) = changes {
            for range in changes.pages.ranges(block) {
                write(base + range.start, &bytes[range]);
            }
        } else {
            // A new renderer or a skipped queued snapshot may have another baseline.
            // Restore this block even if the latest dirty bitmap is empty.
            write(base, bytes);
        }
    }
}

#[cfg(test)]
mod tests;

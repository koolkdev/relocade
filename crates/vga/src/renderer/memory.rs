//! Persistent GPU storage for the captured VGA planes.
use crate::{
    memory::{MemorySnapshot, BLOCK_BYTES},
    DisplaySnapshot,
};
use std::sync::Arc;

pub(super) const BUFFER_BYTES: u64 = 4 * BLOCK_BYTES as u64;

pub(super) struct GpuMemory {
    pub buffer: wgpu::Buffer,
    /// Immutable block identities describing the last queued upload.
    previous: Option<MemorySnapshot>,
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
            previous: None,
        }
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
            // A new renderer or an independently read capture may have another baseline.
            // Restore this block even if the latest dirty bitmap is empty.
            write(base, bytes);
        }
    }
}

#[cfg(test)]
mod tests;

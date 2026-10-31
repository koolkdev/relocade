//! A complete GPU copy of physical VRAM, independent of display layout.
use crate::{memory::PLANE_BYTES, DisplaySnapshot};
use std::sync::Arc;

pub(super) const BUFFER_BYTES: u64 = 4 * PLANE_BYTES as u64;

pub(super) struct GpuMemory {
    pub buffer: wgpu::Buffer,
    /// Immutable plane identities describing the last queued upload.
    previous: Option<[Arc<[u8]>; 4]>,
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
        write_planes(self.previous.as_ref(), snapshot, |offset, bytes| {
            queue.write_buffer(&self.buffer, offset as u64, bytes);
            changed = true;
        });
        self.previous = Some(snapshot.planes.clone());
        changed
    }
}

fn write_planes(
    previous: Option<&[Arc<[u8]>; 4]>,
    snapshot: &DisplaySnapshot,
    mut write: impl FnMut(usize, &[u8]),
) {
    for (plane, bytes) in snapshot.planes.iter().enumerate() {
        let previous = previous.map(|planes| &planes[plane]);
        if previous.is_some_and(|previous| Arc::ptr_eq(previous, bytes)) {
            continue;
        }
        let changes = snapshot.changes.as_ref().filter(|changes| {
            previous.is_some_and(|previous| Arc::ptr_eq(previous, &changes.baseline[plane]))
        });
        let base = plane * PLANE_BYTES;
        if let Some(changes) = changes {
            for range in changes.pages.ranges(plane) {
                write(base + range.start, &bytes[range]);
            }
        } else {
            // A new renderer or a skipped/older capture may have another baseline.
            // Restore this plane even if the latest dirty bitmap is empty.
            write(base, bytes);
        }
    }
}

#[cfg(test)]
mod tests;

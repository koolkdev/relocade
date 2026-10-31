//! A complete GPU copy of physical VRAM, independent of display layout.
use crate::{memory::PLANE_BYTES, DisplaySnapshot};
use std::{ops::Range, sync::Arc};

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

    pub fn upload(&mut self, queue: &wgpu::Queue, snapshot: &DisplaySnapshot) -> [Range<usize>; 4] {
        let bounds = write_planes(self.previous.as_ref(), snapshot, |offset, bytes| {
            queue.write_buffer(&self.buffer, offset as u64, bytes);
        });
        self.previous = Some(snapshot.planes.clone());
        bounds
    }
}

/// Returns changed byte bounds in each plane. An unmatched baseline covers the
/// whole restored plane, so rendering can use the same bounds safely.
fn write_planes(
    previous: Option<&[Arc<[u8]>; 4]>,
    snapshot: &DisplaySnapshot,
    mut write: impl FnMut(usize, &[u8]),
) -> [Range<usize>; 4] {
    let mut bounds: [Range<usize>; 4] = Default::default();
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
            bounds[plane] = changes.writes.bounds(plane);
            for range in changes.writes.ranges(plane) {
                write(base + range.start, &bytes[range]);
            }
        } else {
            // A new renderer or a skipped/older capture may have another baseline.
            // Restore this plane even if the latest dirty bitmap is empty.
            write(base, bytes);
            bounds[plane] = 0..PLANE_BYTES;
        }
    }
    bounds
}

#[cfg(test)]
mod tests;

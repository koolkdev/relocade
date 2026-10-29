//! Persistent GPU backing for physical VGA planes, shared by display layouts.
use super::upload::QueuedBytes;
use crate::{
    damage::{CaptureChanges, PLANE_BYTES, PLANE_ROWS, PLANE_ROW_BYTES},
    DisplaySnapshot, MemoryDamage,
};
use std::ops::Range;

const WORD_BYTES: usize = 4;
const ROW_WORDS: usize = PLANE_ROW_BYTES / WORD_BYTES;

/// Required four-byte words, one bit per word. Display layouts build this only
/// when their memory selection changes; ordinary writes use captured rectangles.
pub(super) struct MemorySelection {
    rows: Box<[[u64; PLANE_ROWS]; 4]>,
}

impl MemorySelection {
    pub fn new() -> Self {
        Self {
            rows: Box::new([[0; PLANE_ROWS]; 4]),
        }
    }

    pub fn clear(&mut self) {
        self.rows.fill([0; PLANE_ROWS]);
    }

    pub fn include_byte(&mut self, plane: usize, address: usize) {
        self.rows[plane][address / PLANE_ROW_BYTES] |=
            1 << (address % PLANE_ROW_BYTES / WORD_BYTES);
    }

    pub fn include_range(&mut self, plane: usize, range: Range<usize>) {
        for word in range.start / WORD_BYTES..range.end.div_ceil(WORD_BYTES) {
            self.include_byte(plane, word * WORD_BYTES);
        }
    }
}

/// The bytes already queued to the GPU and the capture interval they represent.
/// Hidden bytes retain their old contents until a layout selects them again.
pub(super) struct PlaneContents {
    bytes: QueuedBytes,
    previous: Option<CaptureChanges>,
}

impl PlaneContents {
    pub fn new() -> Self {
        Self {
            bytes: QueuedBytes::new(4 * PLANE_BYTES),
            previous: None,
        }
    }

    pub fn update(
        &mut self,
        snapshot: &DisplaySnapshot,
        selection: &MemorySelection,
        selection_changed: bool,
        mut write: impl FnMut(usize, &[u8]),
    ) {
        let damage = if selection_changed {
            // Newly visible memory may have changed while it was unselected.
            MemoryDamage::FULL
        } else {
            CaptureChanges::between(snapshot.changes.as_ref(), self.previous.as_ref())
        };
        for plane in 0..4 {
            let Some(rect) = damage.plane(plane) else {
                continue;
            };
            let first_word = rect.left / WORD_BYTES;
            let end_word = rect.right.div_ceil(WORD_BYTES);
            let columns = (u64::MAX << first_word) & (u64::MAX >> (ROW_WORDS - end_word));
            for row in rect.top..rect.bottom {
                let required = selection.rows[plane][row] & columns;
                if required == 0 {
                    continue;
                }
                let start = row * PLANE_ROW_BYTES;
                self.bytes.update(
                    plane * PLANE_BYTES + start,
                    &snapshot.plane(plane)[start..start + PLANE_ROW_BYTES],
                    required,
                    &mut write,
                );
            }
        }
        self.previous = snapshot.changes.clone();
    }
}

pub(super) struct GpuMemory {
    texture: wgpu::Texture,
    contents: PlaneContents,
    pub view: wgpu::TextureView,
}

impl GpuMemory {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("physical VGA planes"),
            size: wgpu::Extent3d {
                width: ROW_WORDS as u32,
                height: PLANE_ROWS as u32,
                depth_or_array_layers: 4,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {
            texture,
            view,
            contents: PlaneContents::new(),
        }
    }

    pub fn upload(
        &mut self,
        queue: &wgpu::Queue,
        snapshot: &DisplaySnapshot,
        selection: &MemorySelection,
        selection_changed: bool,
    ) {
        self.contents
            .update(snapshot, selection, selection_changed, |offset, bytes| {
                // Selection and comparison already bound every write to one row.
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: (offset % PLANE_ROW_BYTES / WORD_BYTES) as u32,
                            y: (offset % PLANE_BYTES / PLANE_ROW_BYTES) as u32,
                            z: (offset / PLANE_BYTES) as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: None,
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: (bytes.len() / WORD_BYTES) as u32,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
            });
    }
}

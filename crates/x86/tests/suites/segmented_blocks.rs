//! Profile-specialized snapshots preserve per-instruction restart boundaries.

#[path = "segmented_blocks/fetch_boundary.rs"]
mod fetch_boundary;
#[path = "segmented_blocks/profiles.rs"]
mod profiles;
#[path = "segmented_blocks/progress.rs"]
mod progress;

use wasm86_x86::{SegmentAttributes, SegmentDefaultSize, SegmentKind, StoredSegment};

fn code(base: u32, limit: u32, size: SegmentDefaultSize) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        attributes: SegmentAttributes::new(SegmentKind::Code { readable: true }, size),
        ..StoredSegment::flat_code32(0x1b)
    }
}

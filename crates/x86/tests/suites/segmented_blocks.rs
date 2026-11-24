//! Snapshots specialized for fixed-width code preserve per-instruction restart boundaries.

#[path = "segmented_blocks/configurations.rs"]
mod configurations;
#[path = "segmented_blocks/fetch_boundary.rs"]
mod fetch_boundary;
#[path = "segmented_blocks/progress.rs"]
mod progress;

use relocade_x86::{SegmentDefaultSize, SegmentRegister};

fn code(base: u32, limit: u32, size: SegmentDefaultSize) -> SegmentRegister {
    SegmentRegister {
        base,
        limit,
        big: u8::from(size == SegmentDefaultSize::Bits32),
        ..SegmentRegister::flat_code32(0x1b)
    }
}

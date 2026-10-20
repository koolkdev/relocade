//! Profile-specialized snapshots preserve per-instruction restart boundaries.

#[path = "segmented_blocks/fetch_boundary.rs"]
mod fetch_boundary;
#[path = "segmented_blocks/profiles.rs"]
mod profiles;
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

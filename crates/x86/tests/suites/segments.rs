//! Loaded segment caches select, guard and translate data operands.

#[path = "segments/bounds.rs"]
mod bounds;
#[path = "segments/fetch.rs"]
mod fetch;
#[path = "segments/permissions.rs"]
mod permissions;
#[path = "segments/profiles.rs"]
mod profiles;
#[path = "segments/progress.rs"]
mod progress;
#[path = "segments/selection.rs"]
mod selection;
#[path = "segments/stack.rs"]
mod stack;
#[path = "segments/strings.rs"]
mod strings;

use wasm86_x86::StoredSegment;

fn data(base: u32, limit: u32) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        ..StoredSegment::flat_data32(0x23)
    }
}

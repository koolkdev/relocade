//! Loaded segment caches guard and translate instruction and data accesses.

#[path = "segments/bounds.rs"]
mod bounds;
#[path = "segments/code.rs"]
mod code;
#[path = "segments/descriptor_tables.rs"]
mod descriptor_tables;
#[path = "segments/fetch.rs"]
mod fetch;
#[path = "segments/fetch_faults.rs"]
mod fetch_faults;
#[path = "segments/moves.rs"]
mod moves;
#[path = "segments/permissions.rs"]
mod permissions;
#[path = "segments/profiles.rs"]
mod profiles;
#[path = "segments/progress.rs"]
mod progress;
#[path = "segments/push_pop.rs"]
mod push_pop;
#[path = "segments/selection.rs"]
mod selection;
#[path = "segments/selector_cases.rs"]
mod selector_cases;
#[path = "segments/stack.rs"]
mod stack;
#[path = "segments/strings.rs"]
mod strings;
#[path = "segments/transfers.rs"]
mod transfers;

use wasm86_x86::StoredSegment;

fn data(base: u32, limit: u32) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        ..StoredSegment::flat_data32(0x23)
    }
}

fn code(base: u32, limit: u32) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        ..StoredSegment::flat_code32(0x1b)
    }
}

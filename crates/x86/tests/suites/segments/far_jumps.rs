//! Far JMP validates a new code cache and offset before committing either.

#[path = "far_jumps/continuation.rs"]
mod continuation;
#[path = "far_jumps/faults.rs"]
mod faults;
#[path = "far_jumps/fetch.rs"]
mod fetch;
#[path = "far_jumps/sources.rs"]
mod sources;
#[path = "far_jumps/targets.rs"]
mod targets;

use super::{
    data,
    selector_cases::{
        check_one, code_defaults, code_descriptor as descriptor, far_pointer as pointer, loaded,
    },
};
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, SegmentResolution, TestModule},
};
use wasm86_x86::{
    DescriptorTables, PrivilegeLevel, Segment, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorKind, SegmentProfile, StoredSegment,
};

fn immediate(word: bool, offset: u32, selector: u16) -> Vec<u8> {
    let mut code = if word { vec![0x66, 0xea] } else { vec![0xea] };
    code.extend(pointer(word, offset, selector));
    code
}

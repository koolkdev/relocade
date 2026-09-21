//! Far-pointer loads commit a GPR offset and a segment cache together.

#[path = "pointers/continuation.rs"]
mod continuation;
#[path = "pointers/faults.rs"]
mod faults;
#[path = "pointers/loads.rs"]
mod loads;

use super::{
    data,
    selector_cases::{check_one, code_defaults, descriptor, loaded},
};
use crate::{
    register::Gpr32,
    support::{
        blocks::BlockModules,
        machine::{expected, Exit, Image, Step},
        step::{Engine, Event, SegmentResolution, TestModule},
    },
};
use wasm86_x86::{
    DescriptorTables, Segment, SegmentDefaultSize, SegmentDescriptor, SegmentProfile, StoredSegment,
};

const FORMS: [(Segment, &[u8]); 5] = [
    (Segment::Es, &[0xc4]),
    (Segment::Ds, &[0xc5]),
    (Segment::Ss, &[0x0f, 0xb2]),
    (Segment::Fs, &[0x0f, 0xb4]),
    (Segment::Gs, &[0x0f, 0xb5]),
];

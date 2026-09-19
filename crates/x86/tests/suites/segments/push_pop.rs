//! Selector stack transfers use independent memory, operand, and SS widths.

#[path = "push_pop/continuation.rs"]
mod continuation;
#[path = "push_pop/faults.rs"]
mod faults;
#[path = "push_pop/widths.rs"]
mod widths;

use super::{
    data,
    selector_cases::{check_one, code_defaults, descriptor},
};
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, SegmentResolution, TestModule},
};
use wasm86_x86::{
    DescriptorTables, Segment, SegmentAttributes, SegmentDefaultSize, SegmentDescriptor,
    SegmentKind, SegmentProfile, StoredSegment,
};

// Literal encoding order supplies an independent oracle for fixed segment operands.
const PUSH: [(Segment, &[u8]); 6] = [
    (Segment::Es, &[0x06]),
    (Segment::Cs, &[0x0e]),
    (Segment::Ss, &[0x16]),
    (Segment::Ds, &[0x1e]),
    (Segment::Fs, &[0x0f, 0xa0]),
    (Segment::Gs, &[0x0f, 0xa8]),
];
const POP: [(Segment, &[u8]); 5] = [
    (Segment::Es, &[0x07]),
    (Segment::Ss, &[0x17]),
    (Segment::Ds, &[0x1f]),
    (Segment::Fs, &[0x0f, 0xa1]),
    (Segment::Gs, &[0x0f, 0xa9]),
];

fn stack_attributes(big: bool) -> SegmentAttributes {
    SegmentAttributes::new(
        SegmentKind::Data {
            writable: true,
            expand_down: false,
        },
        if big {
            SegmentDefaultSize::Bits32
        } else {
            SegmentDefaultSize::Bits16
        },
    )
}

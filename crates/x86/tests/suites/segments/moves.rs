//! Selector moves cross decoding, host resolution, and CPU commit boundaries.

#[path = "moves/continuation.rs"]
mod continuation;
#[path = "moves/faults.rs"]
mod faults;
#[path = "moves/loads.rs"]
mod loads;
#[path = "moves/stores.rs"]
mod stores;

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
    SegmentDescriptorKind, SegmentProfile, StoredSegment,
};

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
    scenario::Scenario,
    selector_cases::{code_defaults, descriptor},
};

use crate::support::{
    blocks::BlockModules,
    machine::{Exit, Image, Step},
    step::SegmentResolution,
};
use relocade_x86::{
    DescriptorTables, ExecutionProfile, Segment, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorKind, SegmentRegister,
};

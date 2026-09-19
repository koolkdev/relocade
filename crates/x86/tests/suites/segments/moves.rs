//! Selector moves cross decoding, host resolution, and CPU commit boundaries.

#[path = "moves/continuation.rs"]
mod continuation;
#[path = "moves/faults.rs"]
mod faults;
#[path = "moves/loads.rs"]
mod loads;
#[path = "moves/stores.rs"]
mod stores;

use super::data;

use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, SegmentResolution, TestModule},
};
use wasm86_x86::{
    DescriptorTables, Segment, SegmentAttributes, SegmentDefaultSize, SegmentDescriptor,
    SegmentDescriptorKind, SegmentKind, SegmentProfile, StoredSegment,
};

fn check_one(
    engine: Engine,
    profile: SegmentProfile,
    code: &[u8],
    image: &Image,
    resolutions: &[SegmentResolution],
    step: Step<'_>,
) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, 1, profile);
    let mut input = image.input();
    input.segment_resolutions = resolutions.to_vec();
    let mut wanted = expected(image, &[step]);
    for (index, reply) in resolutions.iter().enumerate() {
        wanted.events.insert(
            index,
            Event::ResolveSegment {
                segment: reply.segment as i32,
                selector: i32::from(reply.selector),
            },
        );
    }
    for module in [block, TestModule::interpreter_with_profile(profile)] {
        assert_eq!(
            engine.observe(module, &input, 1),
            wanted,
            "{} {profile:?} {code:02x?}",
            module.entry,
        );
    }
}

fn descriptor(base: u32, size: SegmentDefaultSize) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        0xffff,
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        size,
    )
}

fn code_defaults(image: &mut Image, profile: SegmentProfile) {
    if profile == SegmentProfile::Segmented16 {
        image.cpu.segments.cs.attributes = SegmentAttributes::new(
            SegmentKind::Code { readable: true },
            SegmentDefaultSize::Bits16,
        );
    }
}

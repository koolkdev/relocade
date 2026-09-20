//! Shared admission and observations for instructions that transfer selectors.

use crate::support::{
    blocks::BlockModules,
    machine::{expected, Image, Step},
    step::{Engine, Event, SegmentResolution, TestModule},
};
use wasm86_x86::{
    SegmentAttributes, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind, SegmentKind,
    SegmentProfile, StoredSegment,
};

pub(super) fn check_one(
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

pub(super) fn descriptor(base: u32, size: SegmentDefaultSize) -> SegmentDescriptor {
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

pub(super) fn code_defaults(image: &mut Image, profile: SegmentProfile) {
    if profile == SegmentProfile::Segmented16 {
        image.cpu.segments.cs.attributes = SegmentAttributes::new(
            SegmentKind::Code { readable: true },
            SegmentDefaultSize::Bits16,
        );
    }
}

/// A direct, nonconforming user-code descriptor.
pub(super) fn code_descriptor(
    base: u32,
    limit: u32,
    size: SegmentDefaultSize,
) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        limit,
        SegmentDescriptorKind::Code {
            readable: true,
            conforming: false,
        },
        size,
    )
}

pub(super) fn loaded(selector: u16, base: u32, limit: u32, attributes: u16) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        selector,
        attributes: SegmentAttributes::from_bits(attributes),
    }
}

pub(super) fn far_pointer(word: bool, offset: u32, selector: u16) -> Vec<u8> {
    let mut bytes = offset.to_le_bytes()[..if word { 2 } else { 4 }].to_vec();
    bytes.extend(selector.to_le_bytes());
    bytes
}

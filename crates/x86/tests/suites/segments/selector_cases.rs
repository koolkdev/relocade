//! Shared admission and observations for instructions that transfer selectors.

use crate::support::config::SEGMENTED16;
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Image, Step},
    step::{Event, SegmentResolution, TestModule},
};
use relocade_x86::{
    CompilerConfig, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind, SegmentLimit,
    SegmentRegister,
};

pub(super) fn check_one(
    config: CompilerConfig,
    code: &[u8],
    image: &Image,
    resolutions: &[SegmentResolution],
    step: Step<'_>,
) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, 1, config);
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
    for module in [block, TestModule::interpreter_with_config(config)] {
        assert_eq!(
            module.observe(&input, 1),
            module.expected(wanted.clone()),
            "{} {config:?} {code:02x?}",
            module.entry,
        );
    }
}

pub(super) fn descriptor(base: u32, size: SegmentDefaultSize) -> SegmentDescriptor {
    SegmentDescriptor::new(
        base,
        SegmentLimit::bytes(0xffff).unwrap(),
        SegmentDescriptorKind::Data {
            writable: true,
            expand_down: false,
        },
        size,
    )
}

pub(super) fn code_defaults(image: &mut Image, config: CompilerConfig) {
    if config.entry == SEGMENTED16.entry {
        image.cpu.segments.cs.big = 0;
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
        SegmentLimit::from_effective(limit).unwrap(),
        SegmentDescriptorKind::Code {
            readable: true,
            conforming: false,
        },
        size,
    )
}

pub(super) fn loaded(
    selector: u16,
    base: u32,
    limit: u32,
    properties: SegmentRegister,
) -> SegmentRegister {
    SegmentRegister {
        base,
        limit,
        selector,
        ..properties
    }
}

pub(super) fn far_pointer(word: bool, offset: u32, selector: u16) -> Vec<u8> {
    let mut bytes = offset.to_le_bytes()[..if word { 2 } else { 4 }].to_vec();
    bytes.extend(selector.to_le_bytes());
    bytes
}

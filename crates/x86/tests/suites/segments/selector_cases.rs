//! Shared admission and observations for instructions that transfer selectors.

use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Event, SegmentResolution, TestModule},
};
use relocade_x86::{
    ExecutionProfile, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind, SegmentLimit,
    SegmentProfile, SegmentRegister,
};

pub(super) fn check_one(
    profile: ExecutionProfile,
    code: &[u8],
    image: &Image,
    resolutions: &[SegmentResolution],
    step: Step<'_>,
) {
    check_transfer(profile, code, 1, image, resolutions, step);
}

pub(super) fn check_transfer(
    profile: ExecutionProfile,
    code: &[u8],
    instruction_limit: u32,
    image: &Image,
    resolutions: &[SegmentResolution],
    step: Step<'_>,
) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, instruction_limit, profile);
    let mut input = image.input();
    input.segment_resolutions = resolutions.to_vec();
    let block_exit = match step.exit {
        Exit::Dispatch(_)
            if step.cpu.segments.cs.default_size() != image.cpu.segments.cs.default_size() =>
        {
            Exit::Interpret
        }
        exit => exit,
    };
    for (module, exit) in [
        (block, block_exit),
        (TestModule::interpreter_with_profile(profile), step.exit),
    ] {
        let mut wanted = expected(image, &[Step { exit, ..step }]);
        for (index, reply) in resolutions.iter().enumerate() {
            wanted.events.insert(
                index,
                Event::ResolveSegment {
                    segment: reply.segment as i32,
                    selector: i32::from(reply.selector),
                },
            );
        }
        assert_eq!(
            module.observe(&input, 1),
            module.expected(wanted),
            "{} {profile:?} {code:02x?}",
            module.entry
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

pub(super) fn code_defaults(image: &mut Image, profile: ExecutionProfile) {
    if profile == ExecutionProfile::Protected(SegmentProfile::Segmented16) {
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

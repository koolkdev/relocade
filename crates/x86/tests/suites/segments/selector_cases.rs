//! Shared admission and observations for instructions that transfer selectors.

use crate::support::machine::Image;
use relocade_x86::{
    ExecutionProfile, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind, SegmentLimit,
    SegmentProfile, SegmentRegister,
};

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

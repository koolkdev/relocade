//! Profile requirements shared by state specialization and runtime admission.
use super::{Segment, SegmentDefaultSize};
use crate::{
    state::{access, ExecutionContext, StatePattern},
    ExecutionProfile, SegmentRegister, Segments,
};
pub use relocade_x86_profile::SegmentProfile;
use std::mem::offset_of;

pub(crate) fn assumptions(profile: ExecutionProfile, segment: Segment) -> Vec<StatePattern> {
    let base = offset_of!(ExecutionContext, cpu) + Segments::offset(segment);
    let mut requirements = Vec::new();
    macro_rules! field {
        ($field:ident, $expected:expr) => {
            requirements.push(StatePattern::field(
                access::location(
                    (base + offset_of!(SegmentRegister, $field)) as u32,
                    |cache: &SegmentRegister| &cache.$field,
                ),
                u64::from($expected),
            ));
        };
    }
    macro_rules! attribute {
        ($field:ident, $enabled:expr) => {
            field!($field, u8::from($enabled));
        };
    }
    match profile {
        ExecutionProfile::Real16 => {
            field!(limit, u32::from(u16::MAX));
            attribute!(big, false);
        }
        ExecutionProfile::Protected(profile) => {
            if profile == SegmentProfile::Flat32
                && matches!(
                    segment,
                    Segment::Cs | Segment::Ds | Segment::Es | Segment::Ss
                )
            {
                field!(base, 0u32);
                field!(limit, u32::MAX);
                attribute!(usable, true);
                attribute!(readable, true);
                attribute!(writable, segment != Segment::Cs);
                attribute!(executable, segment == Segment::Cs);
                attribute!(expand_down, false);
                if segment == Segment::Ss {
                    attribute!(big, true);
                }
            }
            if segment == Segment::Cs {
                attribute!(
                    big,
                    SegmentDefaultSize::for_profile(profile.into()) == SegmentDefaultSize::Bits32
                );
            }
        }
    }
    requirements
}

//! Segment requirements shared by state specialization and runtime admission.
use super::{Segment, SegmentDefaultSize};
use crate::{
    state::{access, ExecutionContext, StatePattern},
    EntryAssumptions, SegmentAssumptions, SegmentRegister, Segments,
};
pub use relocade_x86_profile::SegmentProfile;
use std::mem::offset_of;

pub(crate) fn assumptions(entry: EntryAssumptions, segment: Segment) -> Vec<StatePattern> {
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
    match entry.segments {
        SegmentAssumptions::RealMode => {
            field!(limit, u32::from(u16::MAX));
            // A canonical real-mode cache has D/B clear.
            if segment != Segment::Cs || entry.code_default_size.is_none() {
                attribute!(big, false);
            }
        }
        SegmentAssumptions::Flat
            if matches!(
                segment,
                Segment::Cs | Segment::Ds | Segment::Es | Segment::Ss
            ) =>
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
        SegmentAssumptions::Flat | SegmentAssumptions::Runtime => {}
    }
    if let (Segment::Cs, Some(size)) = (segment, entry.code_default_size) {
        attribute!(big, size == SegmentDefaultSize::Bits32);
    }
    requirements
}

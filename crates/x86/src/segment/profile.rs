//! Segment assumptions shared by runtime compatibility and generated cache reads.

use crate::state::SegmentRegister;

use super::{Segment, SegmentDefaultSize};

pub use relocade_x86_profile::SegmentProfile;

/// Facts shared by runtime compatibility checks and generated cache reads.
/// Selectors never constrain these protected-mode profiles.
#[derive(Default)]
pub(crate) struct SegmentAssumptions {
    pub(crate) base: Option<u32>,
    pub(crate) limit: Option<u32>,
    pub(crate) usable: Option<u8>,
    pub(crate) readable: Option<u8>,
    pub(crate) writable: Option<u8>,
    pub(crate) executable: Option<u8>,
    pub(crate) expand_down: Option<u8>,
    pub(crate) big: Option<u8>,
}

impl SegmentAssumptions {
    pub(crate) fn for_profile(profile: SegmentProfile, segment: Segment) -> Self {
        let mut assumptions = Self::default();
        if profile == SegmentProfile::Flat32
            && matches!(
                segment,
                Segment::Cs | Segment::Ds | Segment::Es | Segment::Ss
            )
        {
            assumptions.base = Some(0);
            assumptions.limit = Some(u32::MAX);
            assumptions.usable = Some(1);
            assumptions.readable = Some(1);
            assumptions.writable = Some(u8::from(segment != Segment::Cs));
            assumptions.executable = Some(u8::from(segment == Segment::Cs));
            assumptions.expand_down = Some(0);
            if segment == Segment::Ss {
                assumptions.big = Some(1);
            }
        }
        if segment == Segment::Cs {
            assumptions.big = Some(u8::from(
                SegmentDefaultSize::for_profile(profile.into()) == SegmentDefaultSize::Bits32,
            ));
        }
        assumptions
    }

    pub(crate) fn matches(&self, cache: &SegmentRegister) -> bool {
        self.base.is_none_or(|value| cache.base == value)
            && self.limit.is_none_or(|value| cache.limit == value)
            && self.usable.is_none_or(|value| cache.usable == value)
            && self.readable.is_none_or(|value| cache.readable == value)
            && self.writable.is_none_or(|value| cache.writable == value)
            && self
                .executable
                .is_none_or(|value| cache.executable == value)
            && self
                .expand_down
                .is_none_or(|value| cache.expand_down == value)
            && self.big.is_none_or(|value| cache.big == value)
    }
}

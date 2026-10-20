//! Compatibility of loaded segment state with compilation assumptions.

use crate::state::{SegmentRegister, Segments};

use super::{Segment, SegmentDefaultSize};

/// Protected-mode segment assumptions fixed when an entry is compiled.
/// Used by [`crate::ExecutionProfile::Protected`]; 16-bit code defaults do not
/// select real mode. Entry validity and invalidation follow [`crate::ExecutionProfile`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SegmentProfile {
    /// Flat readable code CS, writable expand-up DS/ES/SS, 32-bit CS defaults and
    /// a 32-bit stack pointer. FS and GS have no assumptions in this profile;
    /// accesses through them use runtime segment handling.
    Flat32,
    /// 32-bit instruction defaults. Segment access and SS.B are runtime inputs.
    Segmented32,
    /// 16-bit instruction defaults. Segment access and SS.B are runtime inputs.
    Segmented16,
}

impl SegmentProfile {
    pub(crate) fn code_default_size(self) -> SegmentDefaultSize {
        match self {
            Self::Flat32 | Self::Segmented32 => SegmentDefaultSize::Bits32,
            Self::Segmented16 => SegmentDefaultSize::Bits16,
        }
    }

    /// Tests the properties assumed by this profile, rather than comparing
    /// selectors or requiring every byte of a segment cache to remain unchanged.
    pub fn is_compatible_with(self, segments: &Segments) -> bool {
        Segment::ALL
            .into_iter()
            .all(|segment| self.assumptions(segment).matches(&segments[segment]))
    }

    pub(crate) fn assumptions(self, segment: Segment) -> SegmentAssumptions {
        let mut assumptions = SegmentAssumptions::default();
        if self == Self::Flat32
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
                self.code_default_size() == SegmentDefaultSize::Bits32,
            ));
        }
        assumptions
    }
}

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

#[cfg(test)]
mod tests;

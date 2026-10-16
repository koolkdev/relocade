//! Compatibility of loaded segment state with compilation assumptions.

use crate::{state::Segments, ExecutionProfile, StoredSegment};

use super::{Segment, SegmentAttributes, SegmentDefaultSize};

#[derive(Clone, Copy, Default)]
pub(crate) struct SegmentAssumptions {
    pub(crate) base: Option<u32>,
    pub(crate) limit: Option<u32>,
    pub(crate) attribute_mask: u16,
    pub(crate) attribute_value: u16,
}

impl SegmentAssumptions {
    pub(crate) fn matches(self, segment: &StoredSegment) -> bool {
        self.base.is_none_or(|base| segment.base == base)
            && self.limit.is_none_or(|limit| segment.limit == limit)
            && segment.attributes.bits() & self.attribute_mask == self.attribute_value
    }
}

impl ExecutionProfile {
    pub(crate) fn segment_assumptions(self, segment: Segment) -> SegmentAssumptions {
        let kind_mask = SegmentAttributes::USABLE
            | SegmentAttributes::CODE
            | SegmentAttributes::READ_WRITE
            | SegmentAttributes::EXPAND_DOWN;
        match self {
            Self::Real16 => SegmentAssumptions {
                base: None,
                limit: Some(0xffff),
                attribute_mask: u16::MAX,
                attribute_value: StoredSegment::real_mode(segment, 0).attributes.bits(),
            },
            Self::Protected(SegmentProfile::Flat32) => {
                let (attribute_mask, attribute_value) = match segment {
                    Segment::Cs => (
                        kind_mask | SegmentAttributes::DEFAULT_BIG,
                        StoredSegment::flat_code32(0).attributes.bits(),
                    ),
                    Segment::Ss => (
                        kind_mask | SegmentAttributes::DEFAULT_BIG,
                        StoredSegment::flat_data32(0).attributes.bits(),
                    ),
                    Segment::Ds | Segment::Es => (
                        kind_mask,
                        StoredSegment::flat_data32(0).attributes.bits() & kind_mask,
                    ),
                    Segment::Fs | Segment::Gs => return SegmentAssumptions::default(),
                };
                SegmentAssumptions {
                    base: Some(0),
                    limit: Some(u32::MAX),
                    attribute_mask,
                    attribute_value,
                }
            }
            Self::Protected(profile) if segment == Segment::Cs => SegmentAssumptions {
                attribute_mask: SegmentAttributes::DEFAULT_BIG,
                attribute_value: if profile.code_default_size() == SegmentDefaultSize::Bits32 {
                    SegmentAttributes::DEFAULT_BIG
                } else {
                    0
                },
                ..SegmentAssumptions::default()
            },
            Self::Protected(_) => SegmentAssumptions::default(),
        }
    }
}

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
        ExecutionProfile::Protected(self).is_compatible_with(segments)
    }
}

#[cfg(test)]
mod tests;

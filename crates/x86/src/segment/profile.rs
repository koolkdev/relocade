//! Compatibility of loaded segment state with compilation assumptions.

use crate::state::{Segments, StoredSegment};

use super::SegmentDefaultSize;

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
        let cs = &segments.cs;
        if cs.big != u8::from(self.code_default_size() == SegmentDefaultSize::Bits32) {
            return false;
        }
        match self {
            Self::Flat32 => {
                segments.ss.big == 1
                    && flat_range(cs)
                    && cs.usable == 1
                    && cs.readable == 1
                    && cs.writable == 0
                    && cs.executable == 1
                    && cs.expand_down == 0
                    && [&segments.ds, &segments.es, &segments.ss]
                        .into_iter()
                        .all(|segment| {
                            flat_range(segment)
                                && segment.usable == 1
                                && segment.readable == 1
                                && segment.writable == 1
                                && segment.executable == 0
                                && segment.expand_down == 0
                        })
            }
            Self::Segmented32 | Self::Segmented16 => true,
        }
    }
}

fn flat_range(segment: &StoredSegment) -> bool {
    segment.base == 0 && segment.limit == u32::MAX
}

#[cfg(test)]
mod tests;

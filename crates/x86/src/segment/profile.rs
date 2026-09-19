//! Compatibility of loaded segment state with compilation assumptions.

use crate::state::{Segments, StoredSegment};

use super::{SegmentDefaultSize, SegmentKind};

/// Segment assumptions under which a compiled entry may execute.
/// Compatibility does not validate instruction-fetch spans, code bytes or mappings and
/// does not perform cache invalidation. The execution owner must invalidate
/// dependent entries and dispatch links when these assumptions cease to hold.
/// A terminal segment load may break compatibility at its cache commit; only
/// publication and dispatch may follow before the next entry is admitted.
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
        if cs.attributes.default_size() != self.code_default_size() {
            return false;
        }
        match self {
            Self::Flat32 => {
                segments.ss.attributes.default_size() == SegmentDefaultSize::Bits32
                    && flat_range(cs)
                    && cs.attributes.kind() == Some(SegmentKind::Code { readable: true })
                    && [&segments.ds, &segments.es, &segments.ss]
                        .into_iter()
                        .all(|segment| {
                            flat_range(segment)
                                && segment.attributes.kind()
                                    == Some(SegmentKind::Data {
                                        writable: true,
                                        expand_down: false,
                                    })
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

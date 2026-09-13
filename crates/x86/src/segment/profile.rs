//! Compatibility of loaded segment state with compilation assumptions.

use crate::state::{Segments, StoredSegment};

use super::{SegmentDefaultSize, SegmentKind};

/// Segment assumptions under which a compiled entry may execute.
/// Compatibility does not establish code-byte or page-mapping validity and
/// does not perform cache invalidation. The execution owner must invalidate
/// dependent entries and dispatch links when these assumptions cease to hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentProfile {
    /// Flat readable code CS, writable expand-up DS/ES/SS, 32-bit CS defaults and
    /// a 32-bit stack pointer. FS and GS have no assumptions in this profile;
    /// accesses through them use runtime segment handling.
    Flat32,
    /// Flat executable CS with 32-bit instruction defaults and a 32-bit stack
    /// pointer. Data-segment ranges and access rights are checked at runtime.
    Segmented32,
}

impl SegmentProfile {
    /// Tests the properties assumed by this profile, rather than comparing
    /// selectors or requiring every byte of a segment cache to remain unchanged.
    pub fn is_compatible_with(self, segments: &Segments) -> bool {
        let cs = &segments.cs;
        if !flat_range(cs)
            || !matches!(cs.attributes.kind(), Some(SegmentKind::Code { .. }))
            || cs.attributes.default_size() != SegmentDefaultSize::Bits32
            || segments.ss.attributes.default_size() != SegmentDefaultSize::Bits32
        {
            return false;
        }
        match self {
            Self::Flat32 => {
                cs.attributes.kind() == Some(SegmentKind::Code { readable: true })
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
            Self::Segmented32 => true,
        }
    }
}

fn flat_range(segment: &StoredSegment) -> bool {
    segment.base == 0 && segment.limit == u32::MAX
}

#[cfg(test)]
mod tests;

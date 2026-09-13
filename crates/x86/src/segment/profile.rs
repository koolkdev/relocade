//! Compatibility of loaded segment state with compilation assumptions.

use crate::state::{Segments, StoredSegment};

use super::{SegmentDefaultSize, SegmentKind};

/// Segment assumptions under which a compiled entry may execute.
/// Compatibility does not establish code-byte or page-mapping validity and
/// does not perform cache invalidation. The execution owner must invalidate
/// dependent entries and dispatch links when these assumptions cease to hold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentProfile {
    /// Flat executable CS, writable expand-up DS/ES/SS, 32-bit CS defaults and
    /// a 32-bit stack pointer. FS and GS have no assumptions in this profile;
    /// any future accesses through them must use runtime segment handling.
    Flat32,
}

impl SegmentProfile {
    /// Tests the properties assumed by this profile, rather than comparing
    /// selectors or requiring every byte of a segment cache to remain unchanged.
    pub fn is_compatible_with(self, segments: &Segments) -> bool {
        match self {
            Self::Flat32 => {
                let cs = &segments.cs;
                flat_range(cs)
                    && matches!(cs.attributes.kind(), Some(SegmentKind::Code { .. }))
                    && cs.attributes.default_size() == SegmentDefaultSize::Bits32
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
                    && segments.ss.attributes.default_size() == SegmentDefaultSize::Bits32
            }
        }
    }
}

fn flat_range(segment: &StoredSegment) -> bool {
    segment.base == 0 && segment.limit == u32::MAX
}

#[cfg(test)]
mod tests;

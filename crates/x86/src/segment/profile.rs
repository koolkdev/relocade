//! Compatibility of loaded segment state with compilation assumptions.

use crate::{ExecutionProfile, Segments};

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
        ExecutionProfile::Protected(self).is_compatible_with(segments)
    }
}

#[cfg(test)]
mod tests;

//! Execution mode and assumptions fixed when an entry is compiled.

use crate::{SegmentDefaultSize, SegmentProfile, Segments};

/// Execution assumptions under which a compiled entry may execute.
/// Compatibility does not validate instruction-fetch spans, code bytes or mappings and
/// does not perform cache invalidation. The execution owner must invalidate
/// dependent entries and dispatch links when these assumptions cease to hold.
/// A terminal segment load may break compatibility at its cache commit; only
/// publication and dispatch may follow before the next entry is admitted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExecutionProfile {
    /// Protected mode at CPL3 under the selected segment assumptions.
    Protected(SegmentProfile),
}

impl ExecutionProfile {
    pub(crate) fn code_default_size(self) -> SegmentDefaultSize {
        match self {
            Self::Protected(profile) => profile.code_default_size(),
        }
    }

    /// Tests cache assumptions within the mode selected by the host. Matching
    /// caches alone do not establish the execution mode; entry and link keys must
    /// retain the profile. Protected profiles ignore irrelevant selector/cache bits.
    pub fn is_compatible_with(self, segments: &Segments) -> bool {
        match self {
            Self::Protected(profile) => profile.is_compatible_with(segments),
        }
    }
}

impl From<SegmentProfile> for ExecutionProfile {
    fn from(profile: SegmentProfile) -> Self {
        Self::Protected(profile)
    }
}

//! Execution mode and assumptions fixed when an entry is compiled.

use crate::{CpuModel, Segment, SegmentDefaultSize, SegmentProfile, Segments, StoredSegment};

/// The processor model and execution assumptions required by a compiled entry.
///
/// Use the complete value in cache and direct-link keys. Blocks and their fallback
/// interpreters must agree on this target, including disabled capabilities.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionTarget {
    pub profile: ExecutionProfile,
    pub cpu_model: CpuModel,
}

impl ExecutionTarget {
    /// Check an entry against the host's active target and loaded segment caches.
    ///
    /// The host must supply its actual execution mode; segment caches alone cannot
    /// establish that mode. Code bytes, fetch permissions and mappings require
    /// separate validation as described by the entry-validity contract.
    pub fn is_compatible_with(self, active: Self, segments: &Segments) -> bool {
        self == active && self.profile.is_compatible_with(segments)
    }
}

/// Execution assumptions under which a compiled entry may execute.
/// Compatibility does not validate instruction-fetch spans, code bytes or mappings and
/// does not perform cache invalidation. The execution owner must invalidate
/// dependent entries and dispatch links when these assumptions cease to hold.
/// A terminal segment load may break compatibility at its cache commit; only
/// publication and dispatch may follow before the next entry is admitted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExecutionProfile {
    /// Protected mode at CPL3 under the selected segment assumptions, with
    /// timestamp reads enabled (CR4.TSD=0).
    Protected(SegmentProfile),
    /// Ordinary real mode: 16-bit defaults, selector-shifted bases, 64 KiB limits
    /// and host-controlled A20. The host establishes canonical caches with
    /// [`Segments::real_mode`]; retained protected-mode caches are not admitted.
    Real16,
}

impl ExecutionProfile {
    /// Fixed CR0 low-word assumptions: MP, ET and NE set; EM and TS clear.
    /// PE follows the execution mode. All remaining low-word bits are reserved.
    pub(crate) fn machine_status_word(self) -> u16 {
        0x32 | u16::from(matches!(self, Self::Protected(_)))
    }

    pub(crate) fn code_default_size(self) -> SegmentDefaultSize {
        match self {
            Self::Protected(profile) => profile.code_default_size(),
            Self::Real16 => SegmentDefaultSize::Bits16,
        }
    }

    /// Tests cache assumptions within the mode selected by the host. Matching
    /// caches alone do not establish the execution mode; entry and link keys must
    /// retain the profile. Protected profiles ignore irrelevant selector/cache bits.
    pub fn is_compatible_with(self, segments: &Segments) -> bool {
        match self {
            Self::Protected(profile) => profile.is_compatible_with(segments),
            Self::Real16 => Segment::ALL.into_iter().all(|segment| {
                let cache = segments[segment];
                cache == StoredSegment::real_mode(segment, cache.selector)
            }),
        }
    }
}

impl From<SegmentProfile> for ExecutionProfile {
    fn from(profile: SegmentProfile) -> Self {
        Self::Protected(profile)
    }
}

#[cfg(test)]
mod tests;

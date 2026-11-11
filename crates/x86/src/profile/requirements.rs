//! CPU-state requirements retained with a generated execution entry.

use crate::{CpuMode, CpuState, ExecutionProfile, PrivilegeLevel, Segment, SegmentDefaultSize};

/// One architectural fact used to select generated behavior.
/// Locally guarded observations and values established by guest writes are not
/// entry requirements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryFact {
    Mode(CpuMode),
    CodeDefaultSize(SegmentDefaultSize),
    PrivilegeLevel(PrivilegeLevel),
    Cr0 { mask: u32, value: u32 },
    Cr4 { mask: u32, value: u32 },
    Segment { segment: Segment, fact: SegmentFact },
}

/// A property of a loaded segment cache, independent of descriptor-table memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentFact {
    Base(u32),
    Limit(u32),
    Usable(bool),
    Readable(bool),
    Writable(bool),
    Executable(bool),
    ExpandDown(bool),
    DefaultSize(SegmentDefaultSize),
}

impl EntryFact {
    pub fn matches(self, cpu: &CpuState) -> bool {
        match self {
            Self::Mode(mode) => cpu.mode() == mode,
            Self::CodeDefaultSize(size) => cpu.code_default_size() == size,
            Self::PrivilegeLevel(level) => cpu.privilege_level() == level,
            Self::Cr0 { mask, value } => cpu.cr0 & mask == value & mask,
            Self::Cr4 { mask, value } => cpu.cr4 & mask == value & mask,
            Self::Segment { segment, fact } => {
                let cache = &cpu.segments[segment];
                match fact {
                    SegmentFact::Base(value) => cache.base == value,
                    SegmentFact::Limit(value) => cache.limit == value,
                    SegmentFact::Usable(value) => (cache.usable != 0) == value,
                    SegmentFact::Readable(value) => (cache.readable != 0) == value,
                    SegmentFact::Writable(value) => (cache.writable != 0) == value,
                    SegmentFact::Executable(value) => (cache.executable != 0) == value,
                    SegmentFact::ExpandDown(value) => (cache.expand_down != 0) == value,
                    SegmentFact::DefaultSize(value) => cache.default_size() == value,
                }
            }
        }
    }
}

/// Admission requirements for one generated entry.
///
/// The baseline profile also covers shared helpers and execution paths that still
/// select behavior directly from the profile. Recorded facts identify individual
/// dependencies as those consumers move to tracked reads. The embedding must
/// preserve these requirements throughout execution; checking them at installation
/// alone does not authorize later state changes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EntryRequirements {
    baseline_profile: Option<ExecutionProfile>,
    facts: Vec<EntryFact>,
}

impl EntryRequirements {
    pub(crate) fn for_profile(profile: ExecutionProfile) -> Self {
        Self {
            baseline_profile: Some(profile),
            facts: Vec::new(),
        }
    }

    /// The conservative profile requirement retained during consumer conversion.
    pub fn baseline_profile(&self) -> Option<ExecutionProfile> {
        self.baseline_profile
    }

    /// Facts consumed by decoding or generated execution, in construction order.
    pub fn facts(&self) -> &[EntryFact] {
        &self.facts
    }

    pub fn matches(&self, cpu: &CpuState) -> bool {
        self.baseline_profile
            .is_none_or(|profile| cpu.is_compatible_with(profile))
            && self.facts.iter().all(|fact| fact.matches(cpu))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SegmentProfile;

    #[test]
    fn baseline_retains_control_and_loaded_cache_admission() {
        let requirements = EntryRequirements::for_profile(SegmentProfile::Flat32.into());
        let mut cpu = CpuState::default();
        assert!(requirements.matches(&cpu));
        cpu.cr0 |= 1 << 3;
        assert!(!requirements.matches(&cpu));
        cpu.cr0 &= !(1 << 3);
        cpu.segments.ds.base = 0x1000;
        assert!(!requirements.matches(&cpu));
    }

    #[test]
    fn facts_match_only_their_architectural_fields() {
        let mut cpu = CpuState::default();
        let timestamp_access = EntryFact::Cr4 { mask: 4, value: 0 };
        cpu.x87.control.rounding_control = 2;
        cpu.cr4 ^= 1 << 9;
        assert!(timestamp_access.matches(&cpu));
        cpu.cr4 |= 4;
        assert!(!timestamp_access.matches(&cpu));
        let readable = EntryFact::Segment {
            segment: Segment::Fs,
            fact: SegmentFact::Readable(true),
        };
        cpu.segments.fs.readable = 0x80;
        assert!(readable.matches(&cpu));
        cpu.segments.fs.readable = 0;
        assert!(!readable.matches(&cpu));
    }
}

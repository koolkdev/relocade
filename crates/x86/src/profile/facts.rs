//! Profile-proven facts and per-entry dependency collection during generation.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, OnceLock},
};

use relocade_compiler::BuildError;

use crate::{
    CpuMode, EntryFact, EntryRequirements, ExecutionProfile, PrivilegeLevel, Segment,
    SegmentDefaultSize, SegmentFact,
};

/// Protected execution retains its effective IF=1/IOPL=0 transfer policy.
/// These are policy values, not predicates over the stored EFLAGS bytes.
pub(crate) enum FlagTransferPolicy {
    Live,
    ProtectedUser,
}

/// Clones share only the dependency record. Execution builders retain their own
/// SSA state and guards when they fork into nested regions.
#[derive(Clone)]
pub(crate) struct ProfileFacts {
    profile: ExecutionProfile,
    recorded: Option<Rc<RefCell<EntryRequirements>>>,
}

impl ProfileFacts {
    pub(crate) fn untracked(profile: ExecutionProfile) -> Self {
        Self {
            profile,
            recorded: None,
        }
    }

    pub(crate) fn tracked(requirements: EntryRequirements) -> Self {
        Self {
            profile: requirements
                .baseline_profile()
                .expect("execution entries have a profile"),
            recorded: Some(Rc::new(RefCell::new(requirements))),
        }
    }

    pub(crate) fn requirements(&self) -> EntryRequirements {
        self.recorded
            .as_ref()
            .expect("a tracked entry")
            .borrow()
            .clone()
    }

    pub(crate) fn profile(&self) -> ExecutionProfile {
        self.mode();
        self.profile
    }

    pub(crate) fn mode(&self) -> CpuMode {
        let mode = match self.profile {
            ExecutionProfile::Real16 => CpuMode::Real,
            ExecutionProfile::Protected(_) => CpuMode::Protected,
        };
        self.record(EntryFact::Mode(mode));
        mode
    }

    pub(crate) fn privilege_level(&self) -> PrivilegeLevel {
        let level = match self.profile {
            ExecutionProfile::Real16 => PrivilegeLevel::Ring0,
            ExecutionProfile::Protected(_) => PrivilegeLevel::Ring3,
        };
        self.record(EntryFact::PrivilegeLevel(level));
        level
    }

    pub(crate) fn allows_io(&self) -> bool {
        // Real mode bypasses IOPL. Protected profiles retain effective IOPL0,
        // independently of the stored IOPL bits; the baseline covers that policy.
        self.mode() == CpuMode::Real || self.privilege_level() == PrivilegeLevel::Ring0
    }

    pub(crate) fn flag_transfer_policy(&self) -> FlagTransferPolicy {
        match self.profile() {
            ExecutionProfile::Real16 => FlagTransferPolicy::Live,
            ExecutionProfile::Protected(_) => {
                self.privilege_level();
                FlagTransferPolicy::ProtectedUser
            }
        }
    }

    pub(crate) fn timestamp_restricted(&self) -> bool {
        // All admitted profiles currently guarantee CR4.TSD=0.
        self.record(EntryFact::Cr4 {
            mask: 1 << 2,
            value: 0,
        });
        false
    }

    pub(crate) fn code_default_size(&self) -> SegmentDefaultSize {
        let size = SegmentDefaultSize::for_profile(self.profile());
        self.record(EntryFact::CodeDefaultSize(size));
        size
    }

    pub(crate) fn segment(&self, segment: Segment, fact: SegmentFact) {
        self.record(EntryFact::Segment { segment, fact });
    }

    fn record(&self, fact: EntryFact) {
        if let Some(recorded) = &self.recorded {
            recorded.borrow_mut().record(fact);
        }
    }
}

/// Each deferred body publishes one result. Read it only after successful
/// compilation; no body waits for another body or shares a mutable collector.
#[derive(Clone)]
pub(crate) struct PendingRequirements {
    baseline: EntryRequirements,
    result: Arc<OnceLock<EntryRequirements>>,
}

impl PendingRequirements {
    pub(crate) fn new(baseline: EntryRequirements) -> Self {
        Self {
            baseline,
            result: Arc::new(OnceLock::new()),
        }
    }

    pub(crate) fn build(
        &self,
        build: impl FnOnce(ProfileFacts) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let facts = ProfileFacts::tracked(self.baseline.clone());
        build(facts.clone())?;
        self.result
            .set(facts.requirements())
            .expect("an entry body is constructed once");
        Ok(())
    }

    pub(crate) fn finish(self) -> EntryRequirements {
        self.result
            .get()
            .expect("successful compilation builds every exported entry")
            .clone()
    }
}

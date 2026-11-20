//! Recompilation policy for self-modifying PC code.

use std::{collections::BTreeSet, sync::Arc};

#[derive(Default)]
pub(super) struct CodeBlacklist {
    previously_invalidated: BTreeSet<u32>,
    pub(super) excluded: ExcludedCode,
}

impl CodeBlacklist {
    /// The first invalidation leaves the byte eligible for recompilation.
    /// A second invalidation of installed code at that byte excludes it permanently.
    pub(super) fn record_invalidation(&mut self, byte: u32) {
        if self.previously_invalidated.contains(&byte) {
            self.excluded.insert(byte);
        } else {
            self.previously_invalidated.insert(byte);
        }
    }
}

/// Backing bytes that writes invalidated in two successive installations.
#[derive(Clone, Default)]
pub(super) struct ExcludedCode(Arc<BTreeSet<u32>>);

impl ExcludedCode {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn intersects(&self, backing: u32, len: usize) -> bool {
        !self.is_empty()
            && self
                .0
                .range(backing..)
                .next()
                .is_some_and(|&byte| ((byte - backing) as usize) < len)
    }

    fn insert(&mut self, byte: u32) {
        if !self.0.contains(&byte) {
            Arc::make_mut(&mut self.0).insert(byte);
        }
    }
}

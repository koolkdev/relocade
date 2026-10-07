//! Retain live control, operations and values, then reclaim their graph storage.

use super::{FunctionGraph, Operation};

mod control;
mod liveness;
mod storage;

pub(super) use storage::Remapping;

#[cfg(test)]
mod tests;

impl FunctionGraph {
    /// Remove unused executions and result channels before placement, preserving
    /// all value and effect IDs. Reuse the reachability snapshot placement needs.
    pub(crate) fn prune_unused(
        &mut self,
        reachable: &[bool],
        observable: impl Fn(&Operation) -> bool,
    ) {
        liveness::prune(self, reachable, observable);
    }

    /// Retain only the live graph after placement releases its ID-based caches.
    /// Resolve control before marking values, then reclaim storage and remap IDs.
    /// Block IDs remain stable for lexical layout and operation origins.
    pub(crate) fn compact(&mut self, observable: impl Fn(&Operation) -> bool) {
        let reachable = self.reachable();
        control::simplify(self, &reachable);
        let retained = liveness::prune(self, &reachable, observable);
        storage::compact(self, &reachable, retained);
    }
}

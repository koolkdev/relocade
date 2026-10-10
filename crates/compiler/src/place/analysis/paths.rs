//! Relate the active context to completed predecessor paths.

use super::ValueAnalysis;
use crate::body::ValueTable;

#[cfg(test)]
mod tests;

/// One completed block's assumptions, published once during function placement.
/// The block identity and its observations remain fixed for every later query.
pub(in crate::place) struct PathSnapshot {
    block: usize,
    analysis: ValueAnalysis,
}

impl PathSnapshot {
    pub(in crate::place) fn analysis(&self) -> &ValueAnalysis {
        &self.analysis
    }
}

impl ValueAnalysis {
    pub(in crate::place) fn snapshot(&self, block: usize) -> PathSnapshot {
        PathSnapshot {
            block,
            analysis: self.clone(),
        }
    }

    /// Prove that a completed predecessor cannot coincide with the active path.
    /// A false answer means no proof was found, not that both paths are reachable.
    /// Both paths belong to this function. Their observations and the existing
    /// value definitions stay fixed during placement, so failed proofs stay valid.
    pub(in crate::place) fn excludes(&self, table: &ValueTable, path: &PathSnapshot) -> bool {
        if path.analysis.context.known.is_empty() {
            return false;
        }
        if let Some(&excluded) = self
            .derived
            .borrow()
            .path_exclusions
            .as_ref()
            .and_then(|paths| paths.get(&path.block))
        {
            return excluded;
        }
        let excluded = path.analysis.conflicts_with(table, self);
        self.derived
            .borrow_mut()
            .path_exclusions
            .get_or_insert_with(Box::default)
            .insert(path.block, excluded);
        excluded
    }
}

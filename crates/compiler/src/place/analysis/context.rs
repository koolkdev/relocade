//! Establish path assumptions and refine them for bounded Boolean case proofs.

use super::{
    comparisons::Comparisons, scoped_map, Bits, DerivedFacts, Range, ScopedMap, ValueAnalysis,
};
use crate::body::ValueTable;

#[derive(Clone, Default)]
pub(super) struct PathContext {
    pub(super) known: ScopedMap<usize, Bits>,
    pub(super) vector_known: Option<Box<ScopedMap<usize, Bits<u128>>>>,
    pub(super) ranges: ScopedMap<usize, Range>,
    pub(super) comparisons: Comparisons,
}

struct Checkpoint {
    known: scoped_map::Checkpoint,
    vectors: Option<scoped_map::Checkpoint>,
    ranges: scoped_map::Checkpoint,
    comparisons: scoped_map::Checkpoint,
}

pub(super) struct SuspendedContext {
    parent: Checkpoint,
    derived: DerivedFacts,
}

/// An entry token belongs to this analysis owner and is left in reverse order.
pub(super) struct ContextScope(usize);

/// Observations used to construct a path; queries cannot add more observations.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::place) enum Assumption {
    Truth { condition: usize, truth: bool },
    Bits { value: usize, mask: u64, bits: u64 },
}

impl PathContext {
    fn checkpoint(&mut self) -> Checkpoint {
        Checkpoint {
            known: self.known.checkpoint(),
            vectors: self.vector_known.as_mut().map(|known| known.checkpoint()),
            ranges: self.ranges.checkpoint(),
            comparisons: self.comparisons.checkpoint(),
        }
    }

    fn restore(&mut self, checkpoint: Checkpoint) {
        self.known.restore(checkpoint.known);
        if let Some(scope) = checkpoint.vectors {
            self.vector_known.as_mut().unwrap().restore(scope);
        } else {
            self.vector_known = None;
        }
        self.ranges.restore(checkpoint.ranges);
        self.comparisons.restore(checkpoint.comparisons);
    }
}

impl ValueAnalysis {
    /// Temporarily refine this proof case, suspending the parent's derived answers.
    pub(super) fn enter(
        &mut self,
        table: &ValueTable,
        assumptions: impl IntoIterator<Item = Assumption>,
    ) -> ContextScope {
        debug_assert!(self.pending.get_mut().is_empty());
        let scope = ContextScope(self.suspended.len());
        let mut assumptions = assumptions.into_iter().peekable();
        if assumptions.peek().is_none() {
            return scope;
        }
        self.suspended.push(SuspendedContext {
            parent: self.context.checkpoint(),
            derived: std::mem::replace(
                self.derived.get_mut(),
                std::mem::take(&mut self.spare_results),
            ),
        });
        self.learn(table, assumptions);
        scope
    }

    pub(super) fn leave(&mut self, scope: ContextScope) {
        debug_assert!(self.pending.get_mut().is_empty());
        if self.suspended.len() == scope.0 {
            return;
        }
        debug_assert_eq!(self.suspended.len(), scope.0 + 1);
        let previous = self.suspended.pop().unwrap();
        self.context.restore(previous.parent);
        self.spare_results
            .recycle(std::mem::replace(self.derived.get_mut(), previous.derived));
    }

    /// Establish observations on an owned context before publishing it for queries.
    pub(in crate::place) fn with_assumptions(
        mut self,
        table: &ValueTable,
        assumptions: impl IntoIterator<Item = Assumption>,
    ) -> Self {
        self.learn(table, assumptions);
        self
    }

    fn learn(&mut self, table: &ValueTable, assumptions: impl IntoIterator<Item = Assumption>) {
        for assumption in assumptions {
            match assumption {
                Assumption::Truth { condition, truth } => self.assume(table, condition, truth),
                Assumption::Bits { value, mask, bits } => self.assume_bits(value, mask, bits),
            }
        }
    }
}

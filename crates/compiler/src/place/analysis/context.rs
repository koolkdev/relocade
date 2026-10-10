//! Establish path assumptions before queries, and suspend the parent on entry.

use super::{
    comparisons::Comparisons, scoped_map, Bits, DerivedFacts, Range, ScopedMap, ValueAnalysis,
};
use crate::body::ValueTable;

#[derive(Clone, Default)]
pub(super) struct PathContext {
    pub(super) known: ScopedMap<usize, Bits>,
    pub(super) ranges: ScopedMap<usize, Range>,
    pub(super) comparisons: Comparisons,
}

struct Checkpoint {
    known: scoped_map::Checkpoint,
    ranges: scoped_map::Checkpoint,
    comparisons: scoped_map::Checkpoint,
}

enum ParentContext {
    Inherited(Checkpoint),
    Replaced(Box<PathContext>),
}

pub(super) struct SuspendedContext {
    parent: ParentContext,
    derived: DerivedFacts,
}

/// An entry token belongs to this analysis owner and is left in reverse order.
pub(in crate::place) struct ContextScope(usize);

/// Observations used to construct a path; queries cannot add more observations.
pub(in crate::place) enum Assumption {
    Truth { condition: usize, truth: bool },
    Bits { value: usize, mask: u64, bits: u64 },
}

impl PathContext {
    fn checkpoint(&mut self) -> Checkpoint {
        Checkpoint {
            known: self.known.checkpoint(),
            ranges: self.ranges.checkpoint(),
            comparisons: self.comparisons.checkpoint(),
        }
    }

    fn restore(&mut self, checkpoint: Checkpoint) {
        self.known.restore(checkpoint.known);
        self.ranges.restore(checkpoint.ranges);
        self.comparisons.restore(checkpoint.comparisons);
    }
}

impl ValueAnalysis {
    /// Select a context and learn all its observations before exposing it to queries.
    /// A block without new knowledge can keep using the current context's results.
    pub(in crate::place) fn enter(
        &mut self,
        table: &ValueTable,
        incoming: Option<Self>,
        assumptions: impl IntoIterator<Item = Assumption>,
    ) -> ContextScope {
        debug_assert!(self.pending.get_mut().is_empty());
        let scope = ContextScope(self.suspended.len());
        let mut assumptions = assumptions.into_iter().peekable();
        if incoming.is_none() && assumptions.peek().is_none() {
            return scope;
        }
        let parent = match incoming {
            Some(incoming) => {
                debug_assert!(incoming.suspended.is_empty());
                ParentContext::Replaced(Box::new(std::mem::replace(
                    &mut self.context,
                    incoming.context,
                )))
            }
            None => ParentContext::Inherited(self.context.checkpoint()),
        };
        self.suspended.push(SuspendedContext {
            parent,
            derived: std::mem::replace(
                self.derived.get_mut(),
                std::mem::take(&mut self.spare_results),
            ),
        });
        self.learn(table, assumptions);
        scope
    }

    pub(in crate::place) fn leave(&mut self, scope: ContextScope) {
        debug_assert!(self.pending.get_mut().is_empty());
        if self.suspended.len() == scope.0 {
            return;
        }
        debug_assert_eq!(self.suspended.len(), scope.0 + 1);
        let previous = self.suspended.pop().unwrap();
        match previous.parent {
            ParentContext::Inherited(checkpoint) => self.context.restore(checkpoint),
            ParentContext::Replaced(context) => self.context = *context,
        }
        self.spare_results
            .recycle(std::mem::replace(self.derived.get_mut(), previous.derived));
    }

    /// A preview owns its observations and answers independently of the live path.
    pub(in crate::place) fn fork(
        &self,
        table: &ValueTable,
        assumptions: impl IntoIterator<Item = Assumption>,
    ) -> Self {
        let mut branch = self.clone();
        branch.learn(table, assumptions);
        branch
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

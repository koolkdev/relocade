//! Refresh demanded answers after refinement, stopping at unchanged results.

use super::{Demand, ScalarFacts, Source};
use crate::body::ValueTable;
use std::num::NonZeroUsize;

impl ScalarFacts {
    pub(super) fn queue(&self, demand: Demand) {
        if let Demand::Constraint(id) = demand {
            self.constraints.changed(id);
            return;
        }
        let mut answers = self.answers.borrow_mut();
        let demand = match demand {
            Demand::Bits(id) if answers.get(&id).is_some_and(|answer| answer.bits.is_some()) => {
                demand
            }
            Demand::Bits(id) | Demand::Reduction(id)
                if answers
                    .get(&id)
                    .is_some_and(|answer| answer.residual.is_some()) =>
            {
                Demand::Reduction(id)
            }
            _ => return,
        };
        if self.updates.borrow_mut().push(demand) {
            if let Demand::Reduction(id) = demand {
                answers.get_or_insert_default(id).revisit = true;
            }
        }
    }

    pub(super) fn changed_fact(&self, source: Source) {
        self.dependencies
            .borrow()
            .users(source, |user| self.queue(user));
    }

    pub(super) fn refined_value(&self, id: usize) {
        if self
            .answers
            .borrow()
            .get(&id)
            .is_some_and(|answer| answer.bits.is_some())
        {
            self.queue(Demand::Bits(id));
        } else {
            // Reduction can have skipped inference before the first assumption.
            self.changed_fact(Source::Bits(id));
        }
        self.queue(Demand::Reduction(id));
    }

    pub(in crate::place) fn refresh_bits(&self, table: &ValueTable) {
        loop {
            let Some(id) = self.updates.borrow_mut().next_bits() else {
                break;
            };
            let before = self
                .answers
                .borrow_mut()
                .get_or_insert_default(id)
                .bits
                .take();
            let after = self.infer_bits(table, id);
            if before != Some(after) {
                self.changed_fact(Source::Bits(id));
            }
        }
    }

    pub(in crate::place) fn next_reduction(&self) -> Option<usize> {
        while let Some(demand) = self.updates.borrow_mut().pop() {
            let Demand::Reduction(id) = demand else {
                unreachable!("inferred facts are settled before reduction")
            };
            if self
                .answers
                .borrow()
                .get(&id)
                .is_some_and(|answer| answer.revisit)
            {
                // A terminal answer can stay dirty until requested. Refreshing it
                // now would materialize constants for completed control predicates.
                let mut consumed = false;
                self.dependencies
                    .borrow()
                    .users(Source::Reduction(id), |user| {
                        let Demand::Reduction(user) = user else {
                            unreachable!()
                        };
                        consumed |= self
                            .answers
                            .borrow()
                            .get(&user)
                            .is_some_and(|answer| answer.residual.is_some());
                    });
                if consumed {
                    return Some(id);
                }
            }
        }
        None
    }

    pub(in crate::place) fn reduced(&self, id: usize) -> Option<usize> {
        self.answers
            .borrow()
            .get(&id)
            .filter(|answer| !answer.revisit)
            .and_then(|answer| answer.residual)
            .map(|id| id.get() - 1)
    }

    pub(in crate::place) fn record_reduction(&mut self, recipe: usize, residual: usize) -> bool {
        self.dependencies.borrow_mut().alias(recipe, residual);
        let answers = self.answers.get_mut();
        let encoded = NonZeroUsize::new(residual + 1).unwrap();
        let answer = answers.get_or_insert_default(recipe);
        let previous = answer.residual.replace(encoded);
        answer.revisit = false;
        answers
            .get_or_insert_default(residual)
            .residual
            .get_or_insert(encoded);
        if previous.is_some() && previous != Some(encoded) {
            self.changed_fact(Source::Reduction(recipe));
        }
        previous != Some(encoded)
    }

    pub(in crate::place) fn aliases(&self, residual: usize) -> Option<usize> {
        self.answers
            .borrow()
            .get(&residual)
            .and_then(|answer| answer.aliases)
            .map(|head| head.get() - 1)
    }

    pub(in crate::place) fn set_aliases(&mut self, residual: usize, head: usize) {
        self.answers
            .get_mut()
            .get_or_insert_default(residual)
            .aliases = NonZeroUsize::new(head + 1);
    }
}

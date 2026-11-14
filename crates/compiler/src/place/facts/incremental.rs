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
        match demand {
            Demand::Bits(id) if answers.get(&id).is_some_and(|answer| answer.bits_complete) => {
                return
            }
            Demand::Reduction(id) if answers.get(&id).is_some_and(|answer| answer.literal) => {
                return
            }
            _ => {}
        }
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
        self.dependencies.borrow().users(source, |user| {
            if self.depends_on(source, user) {
                self.queue(user);
            }
        });
    }

    fn depends_on(&self, source: Source, demand: Demand) -> bool {
        let (Source::Reduction(input), Demand::Reduction(user)) = (source, demand) else {
            return true;
        };
        self.previous_reduction(user)
            .is_none_or(|residual| residual == input)
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
                        if !self.depends_on(Source::Reduction(id), Demand::Reduction(user)) {
                            return;
                        }
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

    // Only rules that inspect expression structure subscribe to its changes.
    pub(in crate::place) fn form(&self, root: usize, mut id: usize) -> usize {
        loop {
            self.dependencies.borrow_mut().form_input(root, id);
            let answers = self.answers.borrow();
            let Some(answer) = answers.get(&id) else {
                return id;
            };
            if let Some(residual) = answer
                .residual
                .map(|id| id.get() - 1)
                .filter(|&residual| residual != id)
            {
                id = residual;
            } else {
                return answer.form.map_or(id, |id| id.get() - 1);
            }
        }
    }

    pub(in crate::place) fn record_form(&mut self, id: usize, form: Option<usize>) {
        let form = form.and_then(|id| NonZeroUsize::new(id + 1));
        let answer = self.answers.get_mut().get_or_insert_default(id);
        if answer.form != form {
            answer.form = form;
            self.changed_fact(Source::Form(id));
        }
    }

    // Stronger facts preserve an equivalence already proved on this path.
    pub(in crate::place) fn previous_reduction(&self, id: usize) -> Option<usize> {
        self.answers
            .borrow()
            .get(&id)
            .and_then(|answer| answer.residual)
            .map(|id| id.get() - 1)
            .filter(|&residual| residual != id)
    }

    pub(in crate::place) fn reduced(&self, id: usize) -> Option<usize> {
        self.answers
            .borrow()
            .get(&id)
            .filter(|answer| !answer.revisit)
            .and_then(|answer| answer.residual)
            .map(|id| id.get() - 1)
    }

    pub(in crate::place) fn record_reduction(
        &mut self,
        recipe: usize,
        residual: usize,
        literal: bool,
    ) -> bool {
        self.dependencies.borrow_mut().alias(recipe, residual);
        let mut answers = self.answers.borrow_mut();
        let encoded = NonZeroUsize::new(residual + 1).unwrap();
        let answer = answers.get_or_insert_default(recipe);
        let previous = answer.residual.replace(encoded);
        answer.revisit = false;
        answer.literal = literal;
        if answers
            .get(&residual)
            .is_none_or(|answer| answer.residual.is_none())
        {
            answers.get_or_insert_default(residual).residual = Some(encoded);
        }
        drop(answers);
        if previous.is_some() && previous != Some(encoded) {
            self.changed_fact(Source::Reduction(recipe));
            self.changed_fact(Source::Form(recipe));
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
            .borrow_mut()
            .get_or_insert_default(residual)
            .aliases = NonZeroUsize::new(head + 1);
    }
}

//! Fold recipes under one set of path facts without scheduling calculations.
use super::*;
use rustc_hash::FxHashMap;

mod equivalences;
use equivalences::Equivalences;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Reducer {
    facts: ScalarFacts,
    residuals: FxHashMap<usize, usize>,
    equivalents: Equivalences,
    pending: Vec<Work>,
}

enum Work {
    Visit(usize),
    Finish(usize),
    Select(usize, usize, usize, usize),
    Alias(usize, usize),
}

/// The observation made by a selected control-flow edge.
pub(super) enum EdgeAssumption {
    Truth { condition: usize, truth: bool },
    Equal { selector: usize, value: u64 },
}

/// Restore the existing fact scope when leaving a dominator subtree.
enum FactScope {
    Inherited(facts::Checkpoint),
    Replaced(Box<ScalarFacts>),
}

pub(super) struct ReductionScope {
    facts: FactScope,
    equivalents: usize,
    replaced: Option<Equivalences>,
}

impl Reducer {
    pub(super) fn from_facts(facts: ScalarFacts) -> Self {
        Self {
            facts,
            ..Self::default()
        }
    }

    pub(super) fn enter(
        &mut self,
        values: &ValueTable,
        incoming: Option<ScalarFacts>,
        edge: Option<EdgeAssumption>,
    ) -> ReductionScope {
        self.residuals.clear();
        let replaced = incoming
            .is_some()
            .then(|| std::mem::take(&mut self.equivalents));
        let facts = match incoming {
            Some(incoming) => {
                FactScope::Replaced(Box::new(std::mem::replace(&mut self.facts, incoming)))
            }
            None => FactScope::Inherited(self.facts.checkpoint()),
        };
        if let Some(edge) = edge {
            match edge {
                EdgeAssumption::Truth { condition, truth } => {
                    self.facts.assume(values, condition, truth)
                }
                EdgeAssumption::Equal { selector, value } => {
                    self.facts
                        .assume_bits(selector, values[selector].ty.mask(), value)
                }
            }
        }
        ReductionScope {
            facts,
            equivalents: self.equivalents.checkpoint(),
            replaced,
        }
    }

    pub(super) fn leave(&mut self, scope: ReductionScope) {
        self.residuals.clear();
        match scope.facts {
            FactScope::Inherited(checkpoint) => self.facts.restore(checkpoint),
            FactScope::Replaced(previous) => self.facts = *previous,
        }
        if let Some(previous) = scope.replaced {
            self.equivalents = previous;
        } else {
            self.equivalents.restore(scope.equivalents);
        }
    }

    pub(super) fn recipes(&self, residual: usize) -> impl Iterator<Item = usize> {
        std::iter::once(residual).chain(self.equivalents.recipes(residual))
    }

    pub(super) fn facts(&self) -> &ScalarFacts {
        &self.facts
    }

    pub(super) fn on_branch(&self, values: &ValueTable, condition: usize, truth: bool) -> Self {
        let mut facts = self.facts.clone();
        facts.assume(values, condition, truth);
        Self::from_facts(facts)
    }

    /// Reduce semantic recipes without consulting execution availability.
    /// Scheduling consumes the result only after all input folds are complete.
    pub(super) fn reduce(&mut self, values: &mut ValueTable, root: usize) -> usize {
        self.pending.push(Work::Visit(root));
        while let Some(task) = self.pending.pop() {
            match task {
                Work::Visit(id) => {
                    if self.residuals.contains_key(&id) {
                        continue;
                    }
                    if let Some(result) = self.known_constant(values, id) {
                        self.record(id, result);
                    } else if let Some(input) = self.facts.bitwise_identity(values, id) {
                        self.pending.push(Work::Alias(id, input));
                        self.pending.push(Work::Visit(input));
                    } else if let Some(result) = values.expression(id) {
                        let expression = result.expression;
                        if let Expression::Select {
                            condition,
                            when_true,
                            when_false,
                        } = expression
                        {
                            self.pending
                                .push(Work::Select(id, condition, when_true, when_false));
                            self.pending.push(Work::Visit(condition));
                        } else {
                            self.pending.push(Work::Finish(id));
                            self.pending
                                .extend(expression.inputs().rev().map(|&v| Work::Visit(v)));
                        }
                    } else {
                        self.residuals.insert(id, id);
                    }
                }
                Work::Select(id, condition, when_true, when_false) => {
                    let condition = self.residuals[&condition];
                    if let Some(bits) = values[condition].scalar_literal() {
                        let input = if bits == 0 { when_false } else { when_true };
                        self.pending.push(Work::Alias(id, input));
                        self.pending.push(Work::Visit(input));
                    } else {
                        self.pending.push(Work::Finish(id));
                        self.pending.push(Work::Visit(when_false));
                        self.pending.push(Work::Visit(when_true));
                    }
                }
                Work::Alias(id, input) => {
                    self.record(id, self.residuals[&input]);
                }
                Work::Finish(id) => {
                    let original = values.expression(id).unwrap();
                    let expression = original.expression.map(|v| self.residuals[v]);
                    let producer = if expression == original.expression {
                        original.producer
                    } else {
                        values.intern(Value {
                            ty: values[original.producer].ty,
                            definition: ValueDefinition::Expression(expression),
                        })
                    };
                    let rewritten = values.expression_result(producer, original.component);
                    // Refold only when an input changed. Construction already
                    // canonicalized the original expression.
                    let result = if let Some(constant) = self.known_constant(values, rewritten) {
                        constant
                    } else if let Some(input) = self.facts.bitwise_identity(values, rewritten) {
                        input
                    } else {
                        let folded = if rewritten == id {
                            id
                        } else {
                            crate::expression::refold(values, rewritten)
                        };
                        self.known_constant(values, folded).unwrap_or(folded)
                    };
                    let result = self.known_constant(values, result).unwrap_or(result);
                    self.record(id, result);
                }
            }
        }
        self.residuals[&root]
    }

    fn known_constant(&self, values: &mut ValueTable, value: usize) -> Option<usize> {
        let bits = self.facts.constant(values, value)?;
        let bits = values.carrier_bits(value, bits)?;
        Some(values.carrier_literal(values[value].ty, bits))
    }

    fn record(&mut self, recipe: usize, residual: usize) {
        self.residuals.insert(recipe, residual);
        if recipe != residual {
            self.equivalents.record(recipe, residual);
        }
    }
}

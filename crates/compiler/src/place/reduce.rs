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

impl Reducer {
    pub(super) fn from_facts(facts: ScalarFacts) -> Self {
        Self {
            facts,
            ..Self::default()
        }
    }

    pub(super) fn recipes(&self, residual: usize) -> impl Iterator<Item = usize> {
        std::iter::once(residual).chain(self.equivalents.recipes(residual))
    }

    pub(super) fn facts(&self) -> &ScalarFacts {
        &self.facts
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
                    let result = if rewritten == id {
                        id
                    } else {
                        crate::expression::refold(values, rewritten)
                    };
                    if result == id {
                        self.record(id, id);
                    } else {
                        // Folding may introduce another recipe. Reduce it in
                        // this context before publishing a stable normal form.
                        self.pending.push(Work::Alias(id, result));
                        self.pending.push(Work::Visit(result));
                    }
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
        self.residuals.entry(residual).or_insert(residual);
        if recipe != residual {
            self.equivalents.record(recipe, residual);
        }
    }
}

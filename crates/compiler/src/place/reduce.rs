//! Fold recipes under one set of path facts without scheduling calculations.
use super::*;

mod equivalences;
use equivalences::Equivalences;
mod observations;
use observations::{Observation, Residuals};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Reducer {
    analysis: ValueAnalysis,
    residuals: Residuals,
    equivalents: Equivalences,
    pending: Vec<Work>,
}

enum Work {
    Visit(Observation),
    Finish(Observation),
    Select(Observation),
    Alias(Observation, Observation),
}

impl Reducer {
    pub(super) fn from_analysis(analysis: ValueAnalysis) -> Self {
        Self {
            analysis,
            ..Self::default()
        }
    }

    pub(super) fn recipes(&self, residual: usize) -> impl Iterator<Item = usize> {
        std::iter::once(residual).chain(self.equivalents.recipes(residual))
    }

    pub(super) fn analysis(&self) -> &ValueAnalysis {
        &self.analysis
    }

    /// Reduce semantic recipes without consulting execution availability.
    /// Scheduling consumes the result only after all input folds are complete.
    pub(super) fn reduce(&mut self, values: &mut ValueTable, root: usize) -> usize {
        self.reduce_observed(values, Observation::full(root))
    }

    fn reduce_observed(&mut self, values: &mut ValueTable, root: Observation) -> usize {
        self.pending.push(Work::Visit(root));
        while let Some(task) = self.pending.pop() {
            match task {
                Work::Visit(observed) => {
                    let id = observed.value;
                    if self.residuals.get(observed).is_some() {
                        continue;
                    }
                    if let Some(result) = self.known_constant(values, observed) {
                        self.record(observed, result);
                    } else if let Some(input) =
                        self.analysis
                            .identity(values, id, self.residuals.mask(observed))
                    {
                        let input = observed.with_value(input);
                        self.pending.push(Work::Alias(observed, input));
                        self.pending.push(Work::Visit(input));
                    } else if values.expression(id).is_some() {
                        let expression = self.residuals.inputs(values, observed);
                        if let Expression::Select { condition, .. } = expression {
                            self.pending.push(Work::Select(observed));
                            self.pending.push(Work::Visit(condition));
                        } else {
                            self.pending.push(Work::Finish(observed));
                            self.pending
                                .extend(expression.inputs().rev().map(|&v| Work::Visit(v)));
                        }
                    } else {
                        self.residuals.record(observed, id);
                    }
                }
                Work::Select(observed) => {
                    let Expression::Select {
                        condition,
                        when_true,
                        when_false,
                    } = self.residuals.inputs(values, observed)
                    else {
                        unreachable!()
                    };
                    let condition = self.residuals.get(condition).unwrap();
                    if let Some(bits) = values[condition].scalar_literal() {
                        let input = if bits == 0 { when_false } else { when_true };
                        self.pending.push(Work::Alias(observed, input));
                        self.pending.push(Work::Visit(input));
                    } else {
                        self.pending.push(Work::Finish(observed));
                        self.pending.push(Work::Visit(when_false));
                        self.pending.push(Work::Visit(when_true));
                    }
                }
                Work::Alias(observed, input) => {
                    self.record(observed, self.residuals.get(input).unwrap());
                }
                Work::Finish(observed) => {
                    let id = observed.value;
                    let original = values.expression(id).unwrap();
                    let inputs = self.residuals.inputs(values, observed);
                    let expression = inputs.map(|&input| self.residuals.get(input).unwrap());
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
                    let result = if let Some(constant) =
                        self.known_constant(values, observed.with_value(rewritten))
                    {
                        constant
                    } else if let Some(input) =
                        self.analysis
                            .identity(values, rewritten, self.residuals.mask(observed))
                    {
                        input
                    } else {
                        let folded = if rewritten == id {
                            id
                        } else {
                            crate::expression::refold(values, rewritten)
                        };
                        self.known_constant(values, observed.with_value(folded))
                            .unwrap_or(folded)
                    };
                    // Ordinary input specialization and refolding run first;
                    // case analysis only sees the remaining small predicate.
                    let result = self
                        .analysis
                        .constant_across_selects(values, result)
                        .map(|bits| values.carrier_literal(crate::Type::I1, bits))
                        .unwrap_or(result);
                    if result == id {
                        self.record(observed, id);
                    } else {
                        // Folding may introduce another recipe. Reduce it in
                        // this context before publishing a stable normal form.
                        let result = observed.with_value(result);
                        self.pending.push(Work::Alias(observed, result));
                        self.pending.push(Work::Visit(result));
                    }
                }
            }
        }
        self.residuals.get(root).unwrap()
    }

    fn known_constant(&self, values: &mut ValueTable, observed: Observation) -> Option<usize> {
        let value = observed.value;
        if !observed.is_full() {
            let bits =
                self.analysis
                    .observed_constant(values, value, self.residuals.mask(observed))?;
            return Some(values.carrier_literal(values[value].ty, bits));
        }
        if !values[value].ty.is_scalar() {
            let bits = self.analysis.vector_constant(values, value)?;
            return Some(values.carrier_literal(values[value].ty, bits));
        }
        let bits = self.analysis.constant(values, value)?;
        let bits = values.carrier_bits(value, bits)?;
        Some(values.carrier_literal(values[value].ty, bits))
    }

    fn record(&mut self, observed: Observation, residual: usize) {
        self.residuals.record(observed, residual);
        if observed.is_full() && observed.value != residual {
            self.equivalents.record(observed.value, residual);
        }
    }
}

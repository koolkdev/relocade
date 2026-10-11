//! Fold recipes under one set of path facts without scheduling calculations.
use super::*;
use analysis::{Assumption, ContextScope};
use rustc_hash::FxHashMap;

mod equivalences;
use equivalences::Equivalences;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Reducer {
    analysis: ValueAnalysis,
    residuals: FxHashMap<usize, usize>,
    equivalents: Equivalences,
    spare: FxHashMap<usize, usize>,
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

impl EdgeAssumption {
    fn observation(self, values: &ValueTable) -> Assumption {
        match self {
            Self::Truth { condition, truth } => Assumption::Truth { condition, truth },
            Self::Equal { selector, value } => Assumption::Bits {
                value: selector,
                mask: values[selector].ty.mask(),
                bits: value,
            },
        }
    }
}

pub(super) struct ReductionScope {
    analysis: ContextScope,
    parent: Option<FxHashMap<usize, usize>>,
    equivalents: usize,
    replaced: Option<Equivalences>,
}

impl Reducer {
    pub(super) fn from_analysis(analysis: ValueAnalysis) -> Self {
        Self {
            analysis,
            ..Self::default()
        }
    }

    pub(super) fn enter(
        &mut self,
        values: &ValueTable,
        incoming: Option<ValueAnalysis>,
        edge: Option<EdgeAssumption>,
    ) -> ReductionScope {
        let observation = edge.map(|edge| edge.observation(values));
        let replaced = incoming
            .is_some()
            .then(|| std::mem::take(&mut self.equivalents));
        let parent = (incoming.is_some() || observation.is_some())
            .then(|| std::mem::replace(&mut self.residuals, std::mem::take(&mut self.spare)));
        ReductionScope {
            equivalents: self.equivalents.checkpoint(),
            parent,
            replaced,
            analysis: self.analysis.enter(values, incoming, observation),
        }
    }

    pub(super) fn leave(&mut self, scope: ReductionScope) {
        if let Some(parent) = scope.parent {
            let mut finished = std::mem::replace(&mut self.residuals, parent);
            finished.clear();
            if finished.capacity() > self.spare.capacity() {
                self.spare = finished;
            }
            if let Some(parent) = scope.replaced {
                self.equivalents = parent;
            } else {
                self.equivalents.restore(scope.equivalents);
            }
        }
        self.analysis.leave(scope.analysis);
    }

    pub(super) fn recipes(&self, residual: usize) -> impl Iterator<Item = usize> {
        std::iter::once(residual).chain(self.equivalents.recipes(residual))
    }

    pub(super) fn analysis(&self) -> &ValueAnalysis {
        &self.analysis
    }

    pub(super) fn on_branch(&self, values: &ValueTable, condition: usize, truth: bool) -> Self {
        let observation = EdgeAssumption::Truth { condition, truth }.observation(values);
        Self {
            analysis: self.analysis.fork(values, [observation]),
            ..Self::default()
        }
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
                    } else if let Some(input) = self.analysis.bitwise_identity(values, id) {
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
                    } else if let Some(input) = self.analysis.bitwise_identity(values, rewritten) {
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
                    // Ordinary input specialization and refolding run first;
                    // case analysis only sees the remaining small predicate.
                    let result = self
                        .analysis
                        .constant_across_selects(values, result)
                        .map(|bits| values.carrier_literal(crate::Type::I1, bits))
                        .unwrap_or(result);
                    self.record(id, result);
                }
            }
        }
        self.residuals[&root]
    }

    fn known_constant(&self, values: &mut ValueTable, value: usize) -> Option<usize> {
        let bits = self.analysis.constant(values, value)?;
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

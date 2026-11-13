//! Fold and lower recipes under fixed path facts before scheduling calculations.
use super::facts::Inference;
use super::*;

mod equivalences;
use equivalences::Equivalences;

#[cfg(test)]
mod tests;

/// One function's path scopes and reusable reduction worklist.
#[derive(Default)]
pub(super) struct Reducer {
    scopes: Vec<Scope>,
    pending: Vec<Work>,
}

#[derive(Clone, Copy)]
pub(super) struct ScopeId(usize);

#[derive(Default)]
struct Scope {
    facts: ScalarFacts,
    equivalents: Equivalences,
}

enum Work {
    Infer(Inference),
    Visit(usize),
    Finish(usize),
    Select(usize, usize, usize, usize),
    Alias(usize, usize),
}

impl Reducer {
    pub(super) fn new(blocks: usize) -> Self {
        Self {
            scopes: Vec::with_capacity(blocks),
            ..Self::default()
        }
    }

    /// Publish a path's facts before its first reduction. Later queries keep them fixed.
    pub(super) fn scope(&mut self, facts: ScalarFacts) -> ScopeId {
        let id = ScopeId(self.scopes.len());
        self.scopes.push(Scope {
            facts,
            ..Scope::default()
        });
        id
    }

    pub(super) fn recipes(
        &self,
        scope: ScopeId,
        residual: usize,
    ) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(residual).chain(
            self.scopes[scope.0]
                .equivalents
                .recipes(residual)
                .iter()
                .copied(),
        )
    }

    pub(super) fn facts(&self, scope: ScopeId) -> &ScalarFacts {
        &self.scopes[scope.0].facts
    }

    /// Reduce semantic recipes without consulting execution availability.
    /// Scheduling consumes the result only after all input folds are complete.
    pub(super) fn reduce(&mut self, values: &mut ValueTable, scope: ScopeId, root: usize) -> usize {
        let scope = &mut self.scopes[scope.0];
        self.pending.push(Work::Visit(root));
        while let Some(task) = self.pending.pop() {
            match task {
                Work::Infer(step) => scope
                    .facts
                    .infer_step(values, step, |step| self.pending.push(Work::Infer(step))),
                Work::Visit(id) => {
                    if scope.facts.reduced(id).is_some() {
                        continue;
                    }
                    if let Some(step) = scope.facts.constant_inference(values, id) {
                        self.pending.push(Work::Visit(id));
                        self.pending.push(Work::Infer(step));
                        continue;
                    }
                    if let Some(result) = scope.known_constant(values, id) {
                        scope.record(id, result);
                    } else if let Some(step) = scope.facts.identity_inference(values, id) {
                        self.pending.push(Work::Visit(id));
                        self.pending.push(Work::Infer(step));
                    } else if let Some(input) = scope.facts.bitwise_identity(values, id) {
                        self.pending.push(Work::Alias(id, input));
                        self.pending.push(Work::Visit(input));
                    } else if let Some(result) = values.expression(id) {
                        let expression = result.expression;
                        if let Expression::Ternary {
                            operator: crate::expression::TernaryOp::Select,
                            first: when_true,
                            second: when_false,
                            third: condition,
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
                        scope.record(id, id);
                    }
                }
                Work::Select(id, condition, when_true, when_false) => {
                    let condition = scope.facts.reduced(condition).unwrap();
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
                    scope.record(id, scope.facts.reduced(input).unwrap());
                }
                Work::Finish(id) => {
                    let original = values.expression(id).unwrap();
                    let expression = original
                        .expression
                        .map(|v| scope.facts.reduced(*v).unwrap());
                    // Construction already canonicalized unchanged expressions.
                    // Fold changed operands directly, without interning a temporary recipe.
                    let result = if expression == original.expression {
                        id
                    } else {
                        crate::expression::fold_carrier(
                            values,
                            values[id].ty,
                            expression,
                            original.component,
                        )
                    };
                    // Consult path facts on newly folded semantic recipes before
                    // expanding them. Lowering uses the same cache and aliases.
                    let result = if result == id {
                        crate::expression::lower(values, id)
                    } else {
                        result
                    };
                    if result == id {
                        // Prove only a stable recipe. Alias/Visit below handles
                        // new or already memoized results without repeating proofs.
                        let result = scope
                            .facts
                            .constant_across_selects(values, id)
                            .map(|bits| values.carrier_literal(crate::Type::I1, bits))
                            .unwrap_or(id);
                        scope.record(id, result);
                    } else {
                        // Folding and lowering may introduce another recipe.
                        // Reduce it in this context before publishing a stable normal form.
                        self.pending.push(Work::Alias(id, result));
                        self.pending.push(Work::Visit(result));
                    }
                }
            }
        }
        scope.facts.reduced(root).unwrap()
    }
}

impl Scope {
    fn known_constant(&self, values: &mut ValueTable, value: usize) -> Option<usize> {
        let bits = self.facts.constant(values, value)?;
        let bits = values.carrier_bits(value, bits)?;
        Some(values.carrier_literal(values[value].ty, bits))
    }

    fn record(&mut self, recipe: usize, residual: usize) {
        self.facts.record_reduction(recipe, residual);
        if recipe != residual {
            self.equivalents.record(recipe, residual);
        }
    }
}

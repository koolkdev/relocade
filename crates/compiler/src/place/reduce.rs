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
    scopes: Vec<ScalarFacts>,
    pending: Vec<Work>,
    equivalents: Equivalences,
}

#[derive(Clone, Copy)]
pub(super) struct ScopeId(usize);

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
        self.scopes.push(facts);
        id
    }

    pub(super) fn recipes(
        &self,
        scope: ScopeId,
        residual: usize,
    ) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(residual).chain(
            self.equivalents
                .recipes(self.scopes[scope.0].aliases(residual)),
        )
    }

    pub(super) fn facts(&self, scope: ScopeId) -> &ScalarFacts {
        &self.scopes[scope.0]
    }

    /// Reduce semantic recipes without consulting execution availability.
    /// Scheduling consumes the result only after all input folds are complete.
    pub(super) fn reduce(&mut self, values: &mut ValueTable, scope: ScopeId, root: usize) -> usize {
        self.scopes[scope.0].refresh_bits(values);
        while let Some(id) = self.scopes[scope.0].next_reduction() {
            self.reduce_value(values, scope, id);
        }
        self.reduce_value(values, scope, root)
    }

    fn reduce_value(&mut self, values: &mut ValueTable, scope: ScopeId, root: usize) -> usize {
        self.pending.push(Work::Visit(root));
        while let Some(task) = self.pending.pop() {
            let facts = &self.scopes[scope.0];
            match task {
                Work::Infer(step) => {
                    facts.infer_step(values, step, |step| self.pending.push(Work::Infer(step)))
                }
                Work::Visit(id) => {
                    if facts.reduced(id).is_some() {
                        continue;
                    }
                    facts.track_reduction(values, id);
                    if let Some(step) = facts.constant_inference(values, id) {
                        self.pending.push(Work::Visit(id));
                        self.pending.push(Work::Infer(step));
                        continue;
                    }
                    if let Some(bits) = facts
                        .constant(values, id)
                        .and_then(|bits| values.carrier_bits(id, bits))
                    {
                        let result = values.carrier_literal(values[id].ty, bits);
                        self.record(values, scope, id, result);
                    } else if let Some(step) = facts.identity_inference(values, id) {
                        self.pending.push(Work::Visit(id));
                        self.pending.push(Work::Infer(step));
                    } else if let Some(input) = facts.bitwise_identity(values, id) {
                        self.pending.push(Work::Alias(id, input));
                        self.pending.push(Work::Visit(input));
                    } else if let Some(residual) = facts.previous_reduction(id) {
                        self.pending.push(Work::Alias(id, residual));
                        self.pending.push(Work::Visit(residual));
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
                        self.record(values, scope, id, id);
                    }
                }
                Work::Select(id, condition, when_true, when_false) => {
                    let condition = facts.reduced(condition).unwrap();
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
                    let result = facts.reduced(input).unwrap();
                    self.record(values, scope, id, result);
                }
                Work::Finish(id) => {
                    let original = values.expression(id).unwrap();
                    let expression = original.expression.map(|v| facts.reduced(*v).unwrap());
                    // An unchanged operand ID can have a new form in this scope.
                    // Pattern matching must inspect that form as well.
                    let mut equivalent = None;
                    let result = {
                        let folded =
                            crate::expression::fold_in_scope(values, id, expression, &|input| {
                                facts.form(id, input)
                            });
                        if folded == id {
                            id
                        } else if values.expression(folded).is_some_and(|result| {
                            result.expression == expression
                                && result.component == original.component
                        }) && facts.constant(values, folded).is_none()
                            && facts.reduced(folded).is_none()
                        {
                            // Operand substitution alone keeps the recipe's identity.
                            equivalent = Some(folded);
                            id
                        } else {
                            folded
                        }
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
                        let result = facts
                            .constant_across_selects(values, id)
                            .map(|bits| values.carrier_literal(crate::Type::I1, bits))
                            .unwrap_or(id);
                        if let Some(equivalent) = equivalent {
                            self.record(values, scope, equivalent, result);
                        }
                        self.record(values, scope, id, result);
                        self.scopes[scope.0].record_form(id, equivalent);
                    } else {
                        // Folding and lowering may introduce another recipe.
                        // Reduce it in this context before publishing a stable normal form.
                        self.pending.push(Work::Alias(id, result));
                        self.pending.push(Work::Visit(result));
                    }
                }
            }
        }
        self.scopes[scope.0].reduced(root).unwrap()
    }

    fn record(&mut self, values: &ValueTable, scope: ScopeId, recipe: usize, residual: usize) {
        let facts = &mut self.scopes[scope.0];
        if facts.record_reduction(
            recipe,
            residual,
            matches!(values[residual].definition, ValueDefinition::Literal(_)),
        ) && recipe != residual
        {
            let head = self.equivalents.record(facts.aliases(residual), recipe);
            facts.set_aliases(residual, head);
        }
    }
}

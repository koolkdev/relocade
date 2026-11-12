//! Fold and lower recipes under fixed path facts before scheduling calculations.
use super::*;
use rustc_hash::FxHashSet;

mod equivalences;
use equivalences::Equivalences;
mod dependencies;
use super::memo::Memo;
pub(super) use dependencies::ReductionGraph;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Reducer {
    facts: ScalarFacts,
    reductions: Memo<usize>,
    equivalents: Equivalences,
    visited: FxHashSet<usize>,
    pending: Vec<Work>,
}

enum Work {
    Visit(usize),
    Finish(usize),
    Select(usize, usize, usize, usize),
    Alias(usize, usize),
}

impl Reducer {
    pub(super) fn fork(&self, facts: ScalarFacts, graph: &mut ReductionGraph) -> Self {
        let changed = graph.changed(&self.facts, &facts);
        let mut reductions = self.reductions.clone();
        for value in changed {
            reductions.remove(value);
        }
        Self {
            facts,
            reductions,
            ..Self::default()
        }
    }

    pub(super) fn from_facts(facts: ScalarFacts) -> Self {
        Self {
            facts,
            ..Self::default()
        }
    }

    pub(super) fn recipes(&self, residual: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(residual).chain(self.equivalents.recipes(residual).iter().copied())
    }

    pub(super) fn facts(&self) -> &ScalarFacts {
        &self.facts
    }

    /// Reduce semantic recipes without consulting execution availability.
    /// Scheduling consumes the result only after all input folds are complete.
    #[cfg(test)]
    pub(super) fn reduce(&mut self, values: &mut ValueTable, root: usize) -> usize {
        self.reduce_in(values, root, &mut ReductionGraph::default())
    }

    pub(super) fn reduce_in(
        &mut self,
        values: &mut ValueTable,
        root: usize,
        graph: &mut ReductionGraph,
    ) -> usize {
        self.pending.push(Work::Visit(root));
        while let Some(task) = self.pending.pop() {
            match task {
                Work::Visit(id) => {
                    if let Some(residual) = self.reductions.get(&id).copied() {
                        if self.visited.insert(id) && id != residual {
                            self.equivalents.record(id, residual);
                        }
                        continue;
                    }
                    if let Some(result) = self.known_constant(values, id) {
                        self.record(values, graph, id, result);
                    } else if let Some(input) = self.facts.bitwise_identity(values, id) {
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
                        self.record(values, graph, id, id);
                    }
                }
                Work::Select(id, condition, when_true, when_false) => {
                    let condition = self.reductions[&condition];
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
                    self.record(values, graph, id, self.reductions[&input]);
                }
                Work::Finish(id) => {
                    let original = values.expression(id).unwrap();
                    let expression = original.expression.map(|v| self.reductions[v]);
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
                        let proof = self.facts.constant_across_selects(values, id);
                        if proof.is_some()
                            && ScalarFacts::default().constant_across_selects(values, id) != proof
                        {
                            graph.case_proof(id);
                        }
                        let result = proof
                            .map(|bits| values.carrier_literal(crate::Type::I1, bits))
                            .unwrap_or(id);
                        self.record(values, graph, id, result);
                    } else {
                        // Folding and lowering may introduce another recipe.
                        // Reduce it in this context before publishing a stable normal form.
                        self.pending.push(Work::Alias(id, result));
                        self.pending.push(Work::Visit(result));
                    }
                }
            }
        }
        self.reductions[&root]
    }

    fn known_constant(&self, values: &mut ValueTable, value: usize) -> Option<usize> {
        let bits = self.facts.constant(values, value)?;
        let bits = values.carrier_bits(value, bits)?;
        Some(values.carrier_literal(values[value].ty, bits))
    }

    fn record(
        &mut self,
        values: &ValueTable,
        graph: &mut ReductionGraph,
        recipe: usize,
        residual: usize,
    ) {
        graph.record(values, recipe, residual);
        self.reductions.insert(recipe, residual);
        if !self.reductions.contains_key(&residual) {
            self.reductions.insert(residual, residual);
        }
        self.visited.insert(recipe);
        self.visited.insert(residual);
        if recipe != residual {
            self.equivalents.record(recipe, residual);
        }
    }
}

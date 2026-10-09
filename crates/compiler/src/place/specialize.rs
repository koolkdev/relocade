//! Fold recipes under one set of path facts without scheduling calculations.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Specializer {
    facts: Facts,
    residuals: HashMap<usize, usize>,
}

/// Restore inherited facts by undoing changes; suspend them when a join replaces them.
pub(super) enum BlockScope {
    Inherited(facts::Checkpoint),
    Replaced(Box<Facts>),
}

pub(super) struct Alias {
    pub(super) recipe: usize,
    pub(super) residual: usize,
}

pub(super) struct Specialization {
    pub(super) value: usize,
    pub(super) aliases: Vec<Alias>,
}

impl Specializer {
    pub(super) fn begin_block(&mut self, incoming: Option<Facts>) -> BlockScope {
        self.residuals.clear();
        match incoming {
            Some(facts) => {
                BlockScope::Replaced(Box::new(std::mem::replace(&mut self.facts, facts)))
            }
            None => BlockScope::Inherited(self.facts.checkpoint()),
        }
    }

    pub(super) fn end_block(&mut self, scope: BlockScope) {
        self.residuals.clear();
        match scope {
            BlockScope::Inherited(checkpoint) => self.facts.restore(checkpoint),
            BlockScope::Replaced(previous) => self.facts = *previous,
        }
    }

    pub(super) fn facts(&self) -> &Facts {
        &self.facts
    }

    pub(super) fn facts_mut(&mut self) -> &mut Facts {
        self.residuals.clear();
        &mut self.facts
    }

    pub(super) fn on_branch(
        &self,
        values: &ValueTable,
        available: &Availability,
        condition: usize,
        truth: bool,
    ) -> Self {
        let mut branch = Self {
            facts: self.facts.clone(),
            residuals: HashMap::new(),
        };
        branch.assume(values, available, condition, truth);
        branch
    }

    pub(super) fn assume(
        &mut self,
        values: &ValueTable,
        available: &Availability,
        condition: usize,
        truth: bool,
    ) {
        // A constant supplies no new runtime observation. Do not replay its
        // shared alias history, including on the discarded edge.
        if matches!(values[condition].definition, ValueDefinition::Literal(_)) {
            return;
        }
        let mut sources = available.sources(values, condition, 1);
        // Learning earlier recipes first reduces invalidation of inference
        // cached for later residuals.
        sources.sort_unstable_by_key(|source| source.recipe);
        let facts = self.facts_mut();
        for source in sources {
            facts.assume(values, source.recipe, truth);
        }
    }

    pub(super) fn equal(
        &mut self,
        values: &ValueTable,
        available: &Availability,
        selector: usize,
        value: u64,
    ) {
        if matches!(values[selector].definition, ValueDefinition::Literal(_)) {
            return;
        }
        let facts = self.facts_mut();
        for source in available.sources(values, selector, values[selector].ty.mask()) {
            facts.assume_bits(source.recipe, source.mask, value);
        }
    }

    /// Resolve to values usable under the current path facts and dominance scope.
    /// Placement may create join parameters here; branch previews query existing
    /// values only. The resolver borrows these facts without changing them.
    /// Each new rewrite reports its alias independently of the memo's lifetime.
    pub(super) fn specialize(
        &mut self,
        graph: &mut FunctionGraph,
        root: usize,
        mut resolve: impl FnMut(&mut FunctionGraph, usize, &Facts) -> Option<usize>,
    ) -> Specialization {
        enum Work {
            Visit(usize),
            Finish(usize),
            Select(usize, usize, usize, usize),
            Alias(usize, usize),
        }
        let mut work = vec![Work::Visit(root)];
        let mut aliases = Vec::new();
        while let Some(task) = work.pop() {
            match task {
                Work::Visit(id) => {
                    if self.residuals.contains_key(&id) {
                        continue;
                    }
                    if let Some(result) = self.known_constant(&mut graph.values, id) {
                        self.record(id, result, &mut aliases);
                    } else if let Some(result) = resolve(graph, id, &self.facts) {
                        self.record(id, result, &mut aliases);
                    } else if let Some(input) = self.facts.bitwise_identity(&graph.values, id) {
                        work.push(Work::Alias(id, input));
                        work.push(Work::Visit(input));
                    } else if let Some(result) = graph.values.expression(id) {
                        let expression = result.expression;
                        if let Expression::Select {
                            condition,
                            when_true,
                            when_false,
                        } = expression
                        {
                            work.push(Work::Select(id, condition, when_true, when_false));
                            work.push(Work::Visit(condition));
                        } else {
                            work.push(Work::Finish(id));
                            work.extend(expression.inputs().rev().map(|&v| Work::Visit(v)));
                        }
                    } else {
                        self.residuals.insert(id, id);
                    }
                }
                Work::Select(id, condition, when_true, when_false) => {
                    let condition = self.residuals[&condition];
                    if let ValueDefinition::Literal(bits) = graph.values[condition].definition {
                        let input = if bits == 0 { when_false } else { when_true };
                        work.push(Work::Alias(id, input));
                        work.push(Work::Visit(input));
                    } else {
                        work.push(Work::Finish(id));
                        work.push(Work::Visit(when_false));
                        work.push(Work::Visit(when_true));
                    }
                }
                Work::Alias(id, input) => {
                    self.record(id, self.residuals[&input], &mut aliases);
                }
                Work::Finish(id) => {
                    let original = graph.values.expression(id).unwrap();
                    let producer = graph.values.intern(Value {
                        ty: graph.values[original.producer].ty,
                        definition: ValueDefinition::Expression(
                            original.expression.map(|v| self.residuals[v]),
                        ),
                    });
                    let rewritten = graph.values.expression_result(producer, original.component);
                    // Rewriting can expose a fact that a joined parameter would
                    // hide. Otherwise, prefer reuse before refolding can rebuild
                    // an equivalent recipe around those inputs.
                    let result =
                        if let Some(constant) = self.known_constant(&mut graph.values, rewritten) {
                            constant
                        } else if let Some(available) = resolve(graph, rewritten, &self.facts) {
                            available
                        } else if let Some(input) =
                            self.facts.bitwise_identity(&graph.values, rewritten)
                        {
                            input
                        } else {
                            let folded = crate::expression::refold(&mut graph.values, rewritten);
                            self.known_constant(&mut graph.values, folded)
                                .or_else(|| resolve(graph, folded, &self.facts))
                                .unwrap_or(folded)
                        };
                    let result = self
                        .known_constant(&mut graph.values, result)
                        .unwrap_or(result);
                    self.record(id, result, &mut aliases);
                }
            }
        }
        Specialization {
            value: self.residuals[&root],
            aliases,
        }
    }

    fn known_constant(&self, values: &mut ValueTable, value: usize) -> Option<usize> {
        let bits = self.facts.constant(values, value)?;
        let bits = values.carrier_bits(value, bits);
        Some(values.carrier_literal(values[value].ty, bits))
    }

    fn record(&mut self, recipe: usize, residual: usize, aliases: &mut Vec<Alias>) {
        self.residuals.insert(recipe, residual);
        if recipe != residual {
            aliases.push(Alias { recipe, residual });
        }
    }
}

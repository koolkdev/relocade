//! Fold recipes under one set of path facts without scheduling calculations.
use super::*;
use std::rc::Rc;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Specializer {
    facts: ValueFacts,
    residuals: HashMap<Observation, usize>,
    demands: Rc<[u128]>,
}

/// A residual preserves these carrier bits of its source recipe.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Observation {
    recipe: usize,
    bits: u128,
}

/// Restore inherited facts by undoing changes; suspend them when a join replaces them.
pub(super) enum BlockScope {
    Inherited(facts::Checkpoint),
    Replaced(Box<ValueFacts>),
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
    pub(super) fn new(demands: Vec<u128>) -> Self {
        Self {
            demands: demands.into(),
            ..Self::default()
        }
    }

    fn observation(&self, values: &ValueTable, recipe: usize, bits: u128) -> Observation {
        Observation {
            recipe,
            bits: (bits | self.demands.get(recipe).copied().unwrap_or(0))
                & values[recipe].ty.carrier().bit_mask(),
        }
    }

    fn inputs(&self, values: &ValueTable, observed: Observation) -> Expression<Observation> {
        let inputs = demand::inputs(values, observed.recipe, observed.bits);
        let mut bits = inputs.inputs();
        values
            .expression(observed.recipe)
            .unwrap()
            .expression
            .map(|&input| self.observation(values, input, *bits.next().unwrap()))
    }

    pub(super) fn begin_block(&mut self, incoming: Option<ValueFacts>) -> BlockScope {
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

    pub(super) fn facts(&self) -> &ValueFacts {
        &self.facts
    }

    pub(super) fn facts_mut(&mut self) -> &mut ValueFacts {
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
            demands: self.demands.clone(),
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
        for source in available.sources(values, selector, values[selector].ty.bit_mask()) {
            facts.assume_bits(source.recipe, source.mask, u128::from(value));
        }
    }

    /// Resolve to values usable under the current path facts and dominance scope.
    /// Placement may create join parameters here; branch previews query existing
    /// values only. The resolver borrows these facts without changing them.
    /// Only complete equivalences escape as aliases. Partial residuals stay in
    /// this path-scoped cache, keyed by the bits they preserve.
    pub(super) fn specialize(
        &mut self,
        graph: &mut FunctionGraph,
        root: usize,
        mut resolve: impl FnMut(&mut FunctionGraph, usize, &ValueFacts) -> Option<usize>,
    ) -> Specialization {
        enum Work {
            Visit(Observation),
            Finish(Observation),
            Select(Observation, Observation, Observation, Observation),
            Alias(Observation, Observation),
        }
        let bits = self
            .demands
            .get(root)
            .copied()
            .unwrap_or_else(|| graph.values[root].ty.carrier().bit_mask());
        let root = self.observation(&graph.values, root, bits);
        let mut work = vec![Work::Visit(root)];
        let mut aliases = Vec::new();
        while let Some(task) = work.pop() {
            match task {
                Work::Visit(observed) => {
                    let id = observed.recipe;
                    if let Some(result) = self.known_constant(&mut graph.values, id) {
                        self.record(&graph.values, observed, result, &mut aliases);
                    } else if let Some(result) = resolve(graph, id, &self.facts) {
                        self.record(&graph.values, observed, result, &mut aliases);
                    } else if self.residuals.contains_key(&observed) {
                        continue;
                    } else if let Some(input) =
                        self.facts.identity(&graph.values, id, observed.bits)
                    {
                        let input = self.observation(&graph.values, input, observed.bits);
                        work.push(Work::Alias(observed, input));
                        work.push(Work::Visit(input));
                    } else if graph.values.expression(id).is_some() {
                        let expression = self.inputs(&graph.values, observed);
                        if let Expression::Select {
                            condition,
                            when_true,
                            when_false,
                        } = expression
                        {
                            work.push(Work::Select(observed, condition, when_true, when_false));
                            work.push(Work::Visit(condition));
                        } else {
                            work.push(Work::Finish(observed));
                            work.extend(expression.inputs().rev().map(|&v| Work::Visit(v)));
                        }
                    } else {
                        self.residuals.insert(observed, id);
                    }
                }
                Work::Select(observed, condition, when_true, when_false) => {
                    let condition = self.residuals[&condition];
                    if let Some(bits) = graph.values[condition].scalar_literal() {
                        let input = if bits == 0 { when_false } else { when_true };
                        work.push(Work::Alias(observed, input));
                        work.push(Work::Visit(input));
                    } else {
                        work.push(Work::Finish(observed));
                        work.push(Work::Visit(when_false));
                        work.push(Work::Visit(when_true));
                    }
                }
                Work::Alias(observed, input) => {
                    self.record(
                        &graph.values,
                        observed,
                        self.residuals[&input],
                        &mut aliases,
                    );
                }
                Work::Finish(observed) => {
                    let id = observed.recipe;
                    let original = graph.values.expression(id).unwrap();
                    let inputs = self.inputs(&graph.values, observed);
                    let expression = inputs.map(|input| self.residuals[input]);
                    let producer = graph.values.intern(Value {
                        ty: graph.values[original.producer].ty,
                        definition: ValueDefinition::Expression(expression),
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
                            self.facts.identity(&graph.values, rewritten, observed.bits)
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
                    self.record(&graph.values, observed, result, &mut aliases);
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
        Some(values.carrier_literal(values[value].ty, bits))
    }

    fn record(
        &mut self,
        values: &ValueTable,
        observed: Observation,
        residual: usize,
        aliases: &mut Vec<Alias>,
    ) {
        self.residuals.insert(observed, residual);
        if observed.recipe != residual
            && observed.bits == values[observed.recipe].ty.carrier().bit_mask()
        {
            aliases.push(Alias {
                recipe: observed.recipe,
                residual,
            });
        }
    }
}

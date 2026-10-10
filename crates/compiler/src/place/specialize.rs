//! Fold recipes under one set of path facts without scheduling calculations.
use super::*;
use analysis::{Assumption, ContextScope};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Specializer {
    analysis: ValueAnalysis,
    residuals: HashMap<usize, usize>,
}

/// The observation made by a selected control-flow edge.
pub(super) enum EdgeAssumption {
    Truth { condition: usize, truth: bool },
    Equal { selector: usize, value: u64 },
}

impl EdgeAssumption {
    fn observations(self, values: &ValueTable, available: &Availability) -> Vec<Assumption> {
        let (observed, mask) = match self {
            Self::Truth { condition, .. } => (condition, 1),
            Self::Equal { selector, .. } => (selector, values[selector].ty.mask()),
        };
        // A literal supplies no runtime observation to replay through its aliases.
        if matches!(values[observed].definition, ValueDefinition::Literal(_)) {
            return Vec::new();
        }
        let mut sources = available.sources(values, observed, mask);
        // Learning earlier recipes first avoids discarding later derivations
        // while the new context's observations are still being propagated.
        sources.sort_unstable_by_key(|source| source.recipe);
        sources
            .into_iter()
            .map(|source| match self {
                Self::Truth { truth, .. } => Assumption::Truth {
                    condition: source.recipe,
                    truth,
                },
                Self::Equal { value, .. } => Assumption::Bits {
                    value: source.recipe,
                    mask: source.mask,
                    bits: value,
                },
            })
            .collect()
    }
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
    pub(super) fn begin_block(
        &mut self,
        values: &ValueTable,
        available: &Availability,
        incoming: Option<ValueAnalysis>,
        edge: Option<EdgeAssumption>,
    ) -> ContextScope {
        // Availability changes across blocks even when path knowledge does not.
        self.residuals.clear();
        let observations = edge
            .map(|edge| edge.observations(values, available))
            .unwrap_or_default();
        self.analysis.enter(values, incoming, observations)
    }

    pub(super) fn end_block(&mut self, scope: ContextScope) {
        self.residuals.clear();
        self.analysis.leave(scope);
    }

    pub(super) fn analysis(&self) -> &ValueAnalysis {
        &self.analysis
    }

    pub(super) fn on_branch(
        &self,
        values: &ValueTable,
        available: &Availability,
        condition: usize,
        truth: bool,
    ) -> Self {
        let observations =
            EdgeAssumption::Truth { condition, truth }.observations(values, available);
        Self {
            analysis: self.analysis.fork(values, observations),
            residuals: HashMap::new(),
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
        mut resolve: impl FnMut(&mut FunctionGraph, usize, &ValueAnalysis) -> Option<usize>,
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
                    } else if let Some(result) = resolve(graph, id, &self.analysis) {
                        self.record(id, result, &mut aliases);
                    } else if let Some(input) = self.analysis.bitwise_identity(&graph.values, id) {
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
                    if let Some(bits) = graph.values[condition].scalar_literal() {
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
                        } else if let Some(available) = resolve(graph, rewritten, &self.analysis) {
                            available
                        } else if let Some(input) =
                            self.analysis.bitwise_identity(&graph.values, rewritten)
                        {
                            input
                        } else {
                            let folded = crate::expression::refold(&mut graph.values, rewritten);
                            self.known_constant(&mut graph.values, folded)
                                .or_else(|| resolve(graph, folded, &self.analysis))
                                .unwrap_or(folded)
                        };
                    let result = self
                        .known_constant(&mut graph.values, result)
                        .unwrap_or(result);
                    // Ordinary input specialization and refolding run first;
                    // case analysis only sees the remaining small predicate.
                    let result = self
                        .analysis
                        .constant_across_selects(&graph.values, result)
                        .map(|bits| graph.values.carrier_literal(crate::Type::I1, bits))
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
        let bits = self.analysis.constant(values, value)?;
        let bits = values.carrier_bits(value, bits)?;
        Some(values.carrier_literal(values[value].ty, bits))
    }

    fn record(&mut self, recipe: usize, residual: usize, aliases: &mut Vec<Alias>) {
        self.residuals.insert(recipe, residual);
        if recipe != residual {
            aliases.push(Alias { recipe, residual });
        }
    }
}

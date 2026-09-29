//! Fold recipes under one set of path facts without scheduling calculations.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Specializer {
    facts: Facts,
    residuals: HashMap<usize, usize>,
}

impl Specializer {
    pub(super) fn residuals(&self) -> impl Iterator<Item = (&usize, &usize)> {
        self.residuals.iter()
    }

    pub(super) fn begin_block(&mut self) -> Facts {
        self.residuals.clear();
        self.facts.clone()
    }

    pub(super) fn facts_mut(&mut self) -> &mut Facts {
        self.residuals.clear();
        &mut self.facts
    }

    pub(super) fn on_branch(&self, graph: &FunctionGraph, condition: usize, truth: bool) -> Self {
        let mut facts = self.facts.clone();
        facts.assume(&graph.values, condition, truth);
        Self {
            facts,
            residuals: HashMap::new(),
        }
    }

    /// Lookup results must be available in this block or preview's dominance
    /// scope. Cache entries belong to that scope and its current facts.
    pub(super) fn specialize(
        &mut self,
        graph: &mut FunctionGraph,
        root: usize,
        mut lookup: impl FnMut(&mut FunctionGraph, usize) -> Option<usize>,
    ) -> usize {
        enum Work {
            Visit(usize),
            Finish(usize),
            Select(usize, usize, usize, usize),
            Alias(usize, usize),
        }
        let mut work = vec![Work::Visit(root)];
        while let Some(task) = work.pop() {
            match task {
                Work::Visit(id) => {
                    if self.residuals.contains_key(&id) {
                        continue;
                    }
                    let value = graph.values[id];
                    if let Some(bits) = self.facts.constant(&graph.values, id) {
                        let bits = graph.values.carrier_bits(id, bits);
                        let result = graph.values.intern(Value {
                            ty: value.ty,
                            definition: ValueDefinition::Constant(bits),
                        });
                        self.residuals.insert(id, result);
                    } else if let Some(result) = lookup(graph, id) {
                        self.residuals.insert(id, result);
                    } else if let ValueDefinition::Expression(expression) = value.definition {
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
                    if let ValueDefinition::Constant(bits) = graph.values[condition].definition {
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
                    self.residuals.insert(id, self.residuals[&input]);
                }
                Work::Finish(id) => {
                    let result =
                        crate::expression::map_inputs(&mut graph.values, id, |v| self.residuals[v]);
                    self.residuals.insert(id, result);
                }
            }
        }
        self.residuals[&root]
    }
}

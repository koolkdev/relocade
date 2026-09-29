//! Schedule shared recipes only when a real downstream demand requires them.
use super::*;
use crate::integer::BinaryOp;
use std::collections::HashSet;

#[cfg(test)]
mod tests;

#[derive(Clone, Default)]
struct Demand {
    common_block: Option<usize>,
    multiple_blocks: bool,
    required_sites: HashSet<usize>,
}

impl Demand {
    fn at(block: usize) -> Self {
        Self {
            common_block: Some(block),
            multiple_blocks: false,
            required_sites: HashSet::from([block]),
        }
    }

    fn merge(&mut self, other: &Self, retained: bool, dominators: &Dominators) {
        let Some(block) = other.common_block else {
            return;
        };
        self.multiple_blocks |= other.multiple_blocks;
        self.common_block = Some(match self.common_block {
            None => block,
            Some(previous) => {
                self.multiple_blocks |= previous != block;
                dominators.common(previous, block).unwrap()
            }
        });
        if retained {
            self.required_sites
                .extend(other.required_sites.iter().copied());
        }
    }
}

pub(super) fn schedules(
    graph: &FunctionGraph,
    reachable: &[bool],
    dominators: &Dominators,
) -> Vec<Vec<usize>> {
    let count = graph.blocks.len();
    let mut successors = successors(graph, reachable);
    successors.push(Vec::new());
    for (source, targets) in successors[..count].iter_mut().enumerate() {
        if !reachable[source] {
            continue;
        }
        if targets.is_empty()
            || targets
                .iter()
                .any(|&target| dominators.dominates(target, source))
        {
            targets.push(count);
        }
    }
    let mut reversed = vec![Vec::new(); count + 1];
    for (source, targets) in successors.iter().enumerate() {
        for &target in targets {
            reversed[target].push(source);
        }
    }
    let postdominators = Dominators::new(count, &reversed, &successors);
    let mut locations = vec![None; graph.effects.len()];
    for (block, data) in graph.blocks.iter().enumerate() {
        for item in &data.items {
            if let BlockItem::Effect(id) = item {
                locations[id.0] = Some(block);
            }
        }
    }
    let mut requirements = vec![None; graph.values.len()];
    for (id, value) in graph.values.iter().enumerate() {
        requirements[id] = match value.definition {
            ValueDefinition::Constant(_) => Some(graph.entry.0),
            ValueDefinition::Parameter { block, .. } => Some(block.0),
            ValueDefinition::Result { effect, .. } => locations[effect.0],
            ValueDefinition::Expression(expression) => {
                let mut requirement = Some(graph.entry.0);
                for &input in expression.inputs() {
                    requirement = match (requirement, requirements[input]) {
                        (Some(a), Some(b)) if dominators.dominates(a, b) => Some(b),
                        (Some(a), Some(b)) if dominators.dominates(b, a) => Some(a),
                        _ => None,
                    };
                }
                requirement
            }
        };
    }
    // Only required sites need individual identities. Other demands contribute
    // their common dominator and whether they span more than one block.
    let mut demands = vec![Demand::default(); graph.values.len()];
    let demand = |value: usize, sites: &Demand, retained: bool, demands: &mut [Demand]| {
        if matches!(
            graph.values[value].definition,
            ValueDefinition::Expression(_)
        ) {
            demands[value].merge(sites, retained, dominators);
        }
    };
    for (block, data) in graph.blocks.iter().enumerate() {
        if !reachable[block] {
            continue;
        }
        let sites = Demand::at(block);
        for item in &data.items {
            if let BlockItem::Effect(id) = item {
                for input in graph.effects[id.0].operation.inputs() {
                    demand(input, &sites, true, &mut demands);
                }
            }
        }
        for input in data.exit.inputs() {
            demand(input, &sites, true, &mut demands);
        }
    }
    let mut schedules = vec![Vec::new(); count];
    for id in (0..graph.values.len()).rev() {
        let ValueDefinition::Expression(expression) = graph.values[id].definition else {
            continue;
        };
        let mut sites = std::mem::take(&mut demands[id]);
        if sites.multiple_blocks {
            let candidate = sites.common_block.unwrap();
            if requirements[id].is_some_and(|required| dominators.dominates(required, candidate))
                && sites
                    .required_sites
                    .iter()
                    .any(|&site| postdominators.dominates(site, candidate))
            {
                schedules[candidate].push(id);
                sites = Demand::at(candidate);
            }
        }
        for &input in expression.inputs() {
            // Most folds can erase an input after path specialization. Only
            // dependencies retained by specialization provide a witness.
            let required = match expression {
                Expression::Binary {
                    operator: BinaryOp::Add,
                    ..
                } => true,
                Expression::Binary {
                    operator: BinaryOp::Mul,
                    left,
                    right,
                } => {
                    let other = if input == left { right } else { left };
                    matches!(graph.values[other].definition, ValueDefinition::Constant(bits) if bits & 1 != 0)
                }
                _ => false,
            };
            demand(input, &sites, required, &mut demands);
        }
    }
    for schedule in &mut schedules {
        schedule.reverse();
    }
    schedules
}

impl Placer<'_> {
    pub(super) fn materialize_shared(&mut self, block: BlockId) {
        let mut scheduled = std::mem::take(&mut self.shared[block.0]);
        if !scheduled.is_empty() {
            self.defer_eliminated(block, &mut scheduled);
        }
        for value in scheduled {
            self.materialize(value, block);
        }
    }

    fn defer_eliminated(&mut self, block: BlockId, scheduled: &mut Vec<usize>) {
        let Exit::If { condition, .. } = self.graph.blocks[block.0].exit else {
            return;
        };
        struct SharedValue {
            recipe: usize,
            residual: usize,
            can_defer: bool,
        }
        let mut values: Vec<_> = scheduled
            .iter()
            .map(|&recipe| SharedValue {
                recipe,
                residual: self.specialize(recipe, block),
                can_defer: false,
            })
            .collect();
        let exit_inputs = self.graph.blocks[block.0].exit.inputs();
        let exit_inputs: Vec<_> = exit_inputs
            .into_iter()
            .map(|value| self.specialize(value, block))
            .collect();
        let exit_needs = self.required_values(exit_inputs.iter().copied());
        for value in &mut values {
            let carrier = self.graph.values.representation(value.residual);
            value.can_defer = !exit_needs.contains(&carrier) && self.needs_evaluation(carrier);
        }
        if !values.iter().any(|value| value.can_defer) {
            return;
        }

        // Previews share the current availability but have separate facts and
        // residual caches. They neither schedule work nor create joined values.
        let mut paths =
            [false, true].map(|truth| self.specializer.on_branch(self.graph, condition, truth));
        for value in &mut values {
            value.can_defer = value.can_defer
                && paths.iter_mut().any(|path| {
                    let residual = path.specialize(self.graph, value.recipe, |_, id| {
                        self.available.get(&id).copied()
                    });
                    !self.needs_evaluation(residual)
                });
        }

        // Retained work may still need a deferred root. Keep that dependency
        // early too, in the original schedule order.
        let needed = self.required_values(
            exit_inputs.into_iter().chain(
                values
                    .iter()
                    .filter(|value| !value.can_defer)
                    .map(|value| value.residual),
            ),
        );
        scheduled.clear();
        scheduled.extend(values.into_iter().filter_map(|value| {
            let carrier = self.graph.values.representation(value.residual);
            (!value.can_defer || needed.contains(&carrier)).then_some(value.recipe)
        }));
    }

    fn needs_evaluation(&self, value: usize) -> bool {
        let value = self.graph.values.representation(value);
        !self.available.contains_key(&value)
            && !matches!(
                self.graph.values[value].definition,
                ValueDefinition::Constant(_) | ValueDefinition::Parameter { .. }
            )
    }

    fn required_values(&self, roots: impl IntoIterator<Item = usize>) -> HashSet<usize> {
        let mut pending: Vec<_> = roots.into_iter().collect();
        let mut needed = HashSet::new();
        while let Some(value) = pending.pop() {
            let value = self.graph.values.representation(value);
            if !needed.insert(value) || self.available.contains_key(&value) {
                continue;
            }
            if let ValueDefinition::Expression(expression) = self.graph.values[value].definition {
                pending.extend(expression.inputs().copied());
            }
        }
        needed
    }
}

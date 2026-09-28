//! Schedule shared recipes only when a real downstream demand requires them.
use super::*;
use crate::integer::BinaryOp;
use std::collections::HashSet;

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

//! Prove where downstream demands permit a shared calculation.
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
    retained_input: bool,
}

impl Demand {
    fn at(block: usize) -> Self {
        Self {
            common_block: Some(block),
            multiple_blocks: false,
            required_sites: HashSet::from([block]),
            retained_input: false,
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
    let mut coverage = Coverage::new(&successors, count);
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
            // Limit collective coverage to retained calculation inputs, to avoid
            // hoisting final publication values across branch decisions.
            if requirements[id].is_some_and(|required| dominators.dominates(required, candidate))
                && (sites
                    .required_sites
                    .iter()
                    .any(|&site| postdominators.dominates(site, candidate))
                    || (sites.retained_input
                        && coverage.all_paths_reach(candidate, &sites.required_sites)))
            {
                schedules[candidate].push(id);
                sites = Demand::at(candidate);
            }
        }
        for &input in expression.inputs() {
            // These operations preserve the operand dependency. Path facts may
            // still eliminate the whole calculation; placement checks that
            // separately before materializing a shared root.
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
            demands[input].retained_input |= required && sites.common_block.is_some();
            demand(input, &sites, required, &mut demands);
        }
    }
    for schedule in &mut schedules {
        schedule.reverse();
    }
    schedules
}

// A set of demands can cover every path even when no single site does.
// Walk only as far as the first demand on each path. Reuse visit marks and
// the worklist so each query neither clears nor copies the whole graph.
// Scoped control flow is reducible: the synthetic exit on every backedge
// ensures that a cycle without a prior demand fails this check.
struct Coverage<'a> {
    successors: &'a [Vec<usize>],
    exit: usize,
    visited: Vec<usize>,
    generation: usize,
    pending: Vec<usize>,
}

impl<'a> Coverage<'a> {
    fn new(successors: &'a [Vec<usize>], exit: usize) -> Self {
        Self {
            successors,
            exit,
            visited: vec![0; successors.len()],
            generation: 0,
            pending: Vec::new(),
        }
    }

    fn all_paths_reach(&mut self, candidate: usize, sites: &HashSet<usize>) -> bool {
        self.generation += 1;
        self.pending.clear();
        self.pending.push(candidate);
        while let Some(block) = self.pending.pop() {
            if sites.contains(&block) || self.visited[block] == self.generation {
                continue;
            }
            if block == self.exit {
                return false;
            }
            self.visited[block] = self.generation;
            self.pending.extend(&self.successors[block]);
        }
        true
    }
}

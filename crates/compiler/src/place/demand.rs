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

    fn branch_placements(
        &mut self,
        required: usize,
        dominators: &Dominators,
        postdominators: &Dominators,
    ) -> Vec<usize> {
        let mut placements = Vec::new();
        if self.required_sites.len() < 2 {
            return placements;
        }
        // Required sites and their common ancestors form a compact dominator
        // tree, avoiding repeated scans of sites through nested bypasses.
        let mut blocks: Vec<_> = self.required_sites.iter().copied().collect();
        blocks.sort_unstable_by_key(|&block| dominators.preorder(block));
        for index in 1..blocks.len() {
            blocks.push(dominators.common(blocks[index - 1], blocks[index]).unwrap());
        }
        blocks.sort_unstable_by_key(|&block| dominators.preorder(block));
        blocks.dedup();

        struct Group {
            block: usize,
            parent: Option<usize>,
            required_count: usize,
            has_witness: bool,
        }
        let mut groups: Vec<Group> = Vec::with_capacity(blocks.len());
        let mut ancestors: Vec<usize> = Vec::new();
        for block in blocks {
            while ancestors
                .last()
                .is_some_and(|&parent| !dominators.dominates(groups[parent].block, block))
            {
                ancestors.pop();
            }
            let required_site = self.required_sites.contains(&block);
            groups.push(Group {
                block,
                parent: ancestors.last().copied(),
                required_count: usize::from(required_site),
                has_witness: required_site,
            });
            ancestors.push(groups.len() - 1);
        }
        for index in (0..groups.len()).rev() {
            let Some(parent) = groups[index].parent else {
                continue;
            };
            // A child's witness also covers its parent exactly when every
            // path from the parent reaches that child first.
            let has_witness = groups[index].has_witness
                && postdominators.dominates(groups[index].block, groups[parent].block);
            let required_count = groups[index].required_count;
            groups[parent].required_count += required_count;
            groups[parent].has_witness |= has_witness;
        }
        let mut covering = None;
        for group in groups {
            if covering.is_some_and(|parent| dominators.dominates(parent, group.block)) {
                self.required_sites.remove(&group.block);
                continue;
            }
            covering = None;
            // A retained consumer elsewhere cannot justify this group. Keep
            // the single-site witness rule for placement within a branch.
            if group.required_count > 1
                && group.has_witness
                && dominators.dominates(required, group.block)
            {
                placements.push(group.block);
                self.required_sites.insert(group.block);
                covering = Some(group.block);
            }
        }
        // Keep the common block for all uses, including uncovered demands.
        // Only the required sites move when a subgroup shares its calculation.
        placements
    }
}

// Sites from all components accumulate at their producer, while each result
// keeps its own use bit so specialization can discard them independently.
#[derive(Clone, Default)]
struct ResultDemand {
    sites: Demand,
    used: bool,
}

pub(super) fn schedules(
    graph: &FunctionGraph,
    reachable: &[bool],
    dominators: &Dominators,
) -> Vec<Vec<usize>> {
    let count = graph.blocks.len();
    let mut coverage = super::coverage::Coverage::new(successors(graph, reachable), dominators);
    let postdominators = coverage.postdominators();
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
            ValueDefinition::Result {
                producer: BlockItem::Effect(effect),
                ..
            } => locations[effect.0],
            ValueDefinition::Result {
                producer: BlockItem::Evaluate(producer),
                ..
            } => requirements[producer],
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
    let mut demands = vec![ResultDemand::default(); graph.values.len()];
    let demand = |value: usize, sites: &Demand, retained: bool, demands: &mut [ResultDemand]| {
        if let Some(result) = graph.values.expression(value) {
            demands[value].used |= sites.common_block.is_some();
            demands[result.producer]
                .sites
                .merge(sites, retained, dominators);
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
        let mut sites = std::mem::take(&mut demands[id].sites);
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
                        && coverage
                            .all_paths_reach(candidate, sites.required_sites.iter().copied())))
            {
                schedules[candidate].extend(
                    graph
                        .values
                        .expression_results(id)
                        .filter(|&result| demands[result].used),
                );
                sites = Demand::at(candidate);
            } else if let Some(required) = requirements[id] {
                for block in sites.branch_placements(required, dominators, &postdominators) {
                    schedules[block].extend(
                        graph
                            .values
                            .expression_results(id)
                            .filter(|&result| demands[result].used),
                    );
                }
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
            if let Some(result) = graph.values.expression(input) {
                demands[result.producer].sites.retained_input |=
                    required && sites.common_block.is_some();
            }
            demand(input, &sites, required, &mut demands);
        }
    }
    for schedule in &mut schedules {
        schedule.reverse();
    }
    schedules
}

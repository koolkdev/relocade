//! Share surviving calculations after path specialization has removed dead uses.

use super::*;
use rustc_hash::FxHashMap;

#[cfg(test)]
mod tests;

struct Calculation {
    first: usize,
    // Allocate only for repeated expressions, then include the first instance.
    instances: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Location {
    block: usize,
    // The original block-item boundary after which this value is available.
    position: usize,
}

/// Share scheduled expressions in a pruned, compacted graph.
/// Return the replacements for `FunctionGraph::replace_values` to apply to all uses.
/// Supplied dominators must describe the graph's current active edges.
pub(super) fn share(
    graph: &mut FunctionGraph,
    dominators: Option<Dominators>,
) -> Option<Vec<usize>> {
    let calculations = equivalent_calculations(graph);
    if calculations
        .iter()
        .all(|calculation| calculation.instances.is_empty())
    {
        return None;
    }
    let mut locations = vec![None; graph.values.len()];
    let mut after_effects = vec![0; graph.blocks.len()];
    for (block, data) in graph.blocks.iter().enumerate() {
        for &parameter in &data.parameters {
            locations[parameter] = Some(Location { block, position: 0 });
        }
        for (position, &item) in data.items.iter().enumerate() {
            if matches!(item, BlockItem::Effect(_)) {
                after_effects[block] = position + 1;
            }
            for result in graph.results(item) {
                locations[result] = Some(Location {
                    block,
                    position: position + 1,
                });
            }
        }
    }
    let reachable = graph.reachable();
    let successors = successors(graph, &reachable);
    let dominators = dominators.unwrap_or_else(|| {
        Dominators::new(graph.entry.0, &successors, &predecessors(graph, &reachable))
    });
    let mut coverage = super::coverage::Coverage::new(successors, &dominators);
    let mut replacements: Vec<_> = (0..graph.values.len()).collect();
    let mut additions = vec![Vec::new(); graph.blocks.len()];
    for calculation in calculations {
        if calculation.instances.len() < 2 {
            continue;
        }
        let sites = calculation
            .instances
            .iter()
            .map(|&id| locations[id].unwrap().block);
        let candidate = sites
            .clone()
            .reduce(|a, b| dominators.common(a, b).unwrap())
            .unwrap();
        if !coverage.all_paths_reach(candidate, sites) {
            continue;
        }
        let existing = calculation
            .instances
            .iter()
            .filter(|&&id| locations[id].unwrap().block == candidate)
            .min_by_key(|&&id| locations[id].unwrap().position);
        let root = *existing.unwrap_or(&calculation.instances[0]);
        let expression = graph
            .values
            .expression(root)
            .unwrap()
            .expression
            .map(|&input| replacements[input]);
        if !calculation.instances.iter().all(|&id| {
            graph
                .values
                .expression(id)
                .unwrap()
                .expression
                .map(|&input| replacements[input])
                == expression
        }) {
            continue;
        }
        // Identical inputs dominate every copy and thus their common dominator.
        // Keep an existing evaluation's position, or follow effects and local inputs.
        let position = if existing.is_some() {
            locations[root].unwrap().position
        } else {
            additions[candidate].push(root);
            expression
                .inputs()
                .filter_map(|&input| locations[input])
                .filter(|location| location.block == candidate)
                .map(|location| location.position)
                .max()
                .unwrap_or(0)
                .max(after_effects[candidate])
        };
        for id in calculation.instances {
            replacements[id] = root;
        }
        locations[root] = Some(Location {
            block: candidate,
            position,
        });
    }
    if replacements
        .iter()
        .enumerate()
        .all(|(id, &replacement)| id == replacement)
    {
        return None;
    }
    for (block, (data, mut additions)) in graph.blocks.iter_mut().zip(additions).enumerate() {
        // Equal boundaries retain dependency order from the bottom-up scan.
        additions.sort_by_key(|&id| locations[id].unwrap().position);
        let mut additions = additions.into_iter().peekable();
        let items = std::mem::take(&mut data.items);
        data.items.reserve(items.len() + additions.len());
        for position in 0..=items.len() {
            while additions
                .peek()
                .is_some_and(|&id| locations[id].unwrap().position == position)
            {
                data.items
                    .push(BlockItem::Evaluate(additions.next().unwrap()));
            }
            if let Some(&item) = items.get(position) {
                if !matches!(item, BlockItem::Evaluate(id)
                    if replacements[id] != id || locations[id].unwrap().block != block)
                {
                    data.items.push(item);
                }
            }
        }
    }
    // Both copies had identical rewritten inputs, so sharing cannot make an
    // input dead. Only replaced definitions need to be removed from storage.
    Some(replacements)
}

// Producers follow their inputs in the value table. Number equivalent residuals
// bottom-up, then separately prove that an actual definition can be shared.
fn equivalent_calculations(graph: &FunctionGraph) -> Vec<Calculation> {
    let mut numbers: Vec<_> = (0..graph.values.len()).collect();
    let mut keys = FxHashMap::default();
    let mut calculations: Vec<Calculation> = Vec::new();
    for id in 0..graph.values.len() {
        let ValueDefinition::Expression(expression) = graph.values[id].definition else {
            continue;
        };
        // Different result channels can lower to different amounts of work.
        // Sharing a tuple would force unused components onto some paths.
        if graph.values.expression_results(id).len() != 1 {
            continue;
        }
        let key = Value {
            ty: graph.values[id].ty,
            definition: ValueDefinition::Expression(expression.map(|&input| numbers[input])),
        };
        let index = *keys.entry(key).or_insert_with(|| {
            calculations.push(Calculation {
                first: id,
                instances: Vec::new(),
            });
            calculations.len() - 1
        });
        let calculation = &mut calculations[index];
        numbers[id] = calculation.first;
        if calculation.first != id {
            if calculation.instances.is_empty() {
                calculation.instances.push(calculation.first);
            }
            calculation.instances.push(id);
        }
    }
    calculations
}

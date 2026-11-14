//! Expand surviving semantic operations without reopening path analysis.
use super::*;

pub(super) fn run(graph: &mut SsaGraph, dominators: &Dominators, changed: &mut Vec<bool>) {
    let original_count = graph.values.len();
    let mut aliases: Vec<_> = (0..original_count).collect();
    let mut emitted = vec![true; original_count];
    let mut order: Vec<_> = (0..graph.blocks.len()).collect();
    order.sort_by_key(|&block| dominators.rank[block]);
    for block in order {
        let items = std::mem::take(&mut graph.blocks[block].items);
        for item in items {
            let BlockItem::Evaluate(id) = item else {
                graph.append(BlockId(block), item);
                continue;
            };
            let result = crate::expression::lower(&mut graph.values, id);
            if result == id {
                graph.append(BlockId(block), item);
                continue;
            }
            while aliases.len() < graph.values.len() {
                aliases.push(aliases.len());
                emitted.push(false);
            }
            aliases[id] = result;
            let mut work = vec![(result, false)];
            while let Some((id, ready)) = work.pop() {
                let Some(result) = graph.values.expression(id) else {
                    continue;
                };
                if emitted[result.producer] {
                    continue;
                }
                if ready {
                    emitted[result.producer] = true;
                    graph.append(BlockId(block), BlockItem::Evaluate(result.producer));
                } else {
                    work.push((id, true));
                    work.extend(
                        result
                            .expression
                            .inputs()
                            .rev()
                            .map(|&input| (input, false)),
                    );
                }
            }
        }
    }
    for id in 0..aliases.len() {
        let mut result = aliases[id];
        while result != aliases[result] {
            result = aliases[result];
        }
        aliases[id] = result;
    }
    changed.resize(graph.values.len(), false);
    graph.rewrite_inputs(|input| aliases[input], |id| changed[id] = true);
}

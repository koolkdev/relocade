//! Remove repeated branch tests before they can force unnecessary value copies.
use super::*;

pub(super) fn fold_repeated(
    graph: &mut FunctionGraph,
    dominators: &Dominators,
    incoming: &[Vec<usize>],
) -> bool {
    let mut uses = vec![0_u8; graph.values.len()];
    let branches: Vec<_> = graph
        .blocks
        .iter()
        .map(|block| {
            if let Exit::If {
                condition,
                ref taken,
                ref otherwise,
            } = block.exit
            {
                if graph.values[condition].scalar_literal().is_none() {
                    uses[condition] = uses[condition].saturating_add(1);
                    return Some((condition, taken.target, otherwise.target));
                }
            }
            None
        })
        .collect();
    if !uses.iter().any(|&count| count > 1) {
        return false;
    }
    let mut children = vec![Vec::new(); graph.blocks.len()];
    for (block, parent) in dominators.parent.iter().enumerate() {
        if let Some(parent) = parent {
            if *parent != block {
                children[*parent].push(block);
            }
        }
    }
    let mut truths = vec![None; uses.len()];
    let mut changes = Vec::new();
    let mut work = vec![(graph.entry.0, None)];
    let mut changed = false;
    while let Some((block, checkpoint)) = work.pop() {
        if let Some(checkpoint) = checkpoint {
            while changes.len() > checkpoint {
                let (condition, previous) = changes.pop().unwrap();
                truths[condition] = previous;
            }
            continue;
        }
        let checkpoint = changes.len();
        if let [source] = incoming[block][..] {
            if let Some((condition, taken, otherwise)) = branches[source] {
                if uses[condition] > 1 && taken != otherwise {
                    let previous = truths[condition].replace(taken.0 == block);
                    changes.push((condition, previous));
                }
            }
        }
        if let Some((condition, _, _)) = branches[block] {
            if let Some(truth) = truths[condition] {
                let literal = graph.values.literal(crate::Type::I1, u64::from(truth));
                if let Exit::If { condition, .. } = &mut graph.blocks[block].exit {
                    *condition = literal;
                }
                changed = true;
            }
        }
        work.push((block, Some(checkpoint)));
        work.extend(children[block].iter().map(|&child| (child, None)));
    }
    changed
}

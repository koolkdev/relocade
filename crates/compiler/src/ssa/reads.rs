//! Keep memory snapshots before conflicting effects; sink through unambiguous routes.
use super::*;
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Demand {
    None,
    At { block: BlockId, position: usize },
    Several,
}
impl Demand {
    fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, x) | (x, Self::None) => x,
            (
                Self::At {
                    block: a,
                    position: x,
                },
                Self::At {
                    block: b,
                    position: y,
                },
            ) if a == b => Self::At {
                block: a,
                position: x.min(y),
            },
            _ => Self::Several,
        }
    }
}

pub(super) fn prepare(graph: &mut FunctionGraph, summaries: &[Effects], reachable: &[bool]) {
    let mut demands = vec![Demand::None; graph.values.len()];
    let mut effects = vec![Demand::None; graph.effects.len()];
    let mut work = VecDeque::new();
    fn update(id: usize, demand: Demand, demands: &mut [Demand], work: &mut VecDeque<usize>) {
        let merged = demands[id].union(demand);
        if merged != demands[id] {
            demands[id] = merged;
            work.push_back(id);
        }
    }
    let mut positions = vec![0; graph.effects.len()];
    for (index, block) in graph.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for (position, item) in block.items.iter().enumerate() {
            let demand = Demand::At {
                block: BlockId(index),
                position,
            };
            let BlockItem::Effect(id) = item else {
                continue;
            };
            positions[id.0] = position;
            if effects::observable(&graph.effects[id.0].operation, summaries) {
                effects[id.0] = demand;
                for input in graph.effects[id.0].operation.inputs() {
                    update(input, demand, &mut demands, &mut work);
                }
            }
        }
        for input in block.exit.inputs() {
            update(
                input,
                Demand::At {
                    block: BlockId(index),
                    position: block.items.len(),
                },
                &mut demands,
                &mut work,
            );
        }
    }
    while let Some(value) = work.pop_front() {
        match graph.producer_of(value) {
            Some(producer @ BlockItem::Evaluate(_)) => {
                let demand = demands[value];
                for input in graph.inputs(producer) {
                    update(input, demand, &mut demands, &mut work);
                }
            }
            Some(BlockItem::Effect(effect)) => {
                let merged = effects[effect.0].union(demands[value]);
                if merged == effects[effect.0] {
                    continue;
                }
                effects[effect.0] = merged;
                let producer = &graph.effects[effect.0];
                for input in producer.operation.inputs() {
                    update(
                        input,
                        Demand::At {
                            block: producer.origin,
                            position: positions[effect.0],
                        },
                        &mut demands,
                        &mut work,
                    );
                }
            }
            _ => {}
        }
    }
    let predecessors = predecessors(graph, reachable);
    let mut moves = vec![Vec::new(); graph.blocks.len()];
    let mut remove = vec![false; graph.effects.len()];
    for (index, producer) in graph.effects.iter().enumerate() {
        if effects::observable(&producer.operation, summaries) {
            continue;
        }
        match effects[index] {
            Demand::None => remove[index] = true,
            Demand::At {
                block: target,
                position,
            } if target == producer.origin => {
                let origin = positions[index];
                let items = &graph.blocks[target.0].items;
                let end = (origin + 1..position)
                    .find(|&at| blocks_read(graph, summaries, EffectId(index), items[at]))
                    .unwrap_or(position);
                if end > origin {
                    remove[index] = true;
                    moves[target.0].push((end, BlockItem::Effect(EffectId(index))));
                }
            }
            Demand::At { block: target, .. }
                if can_sink(graph, summaries, EffectId(index), target, &predecessors) =>
            {
                remove[index] = true;
                moves[target.0].push((0, BlockItem::Effect(EffectId(index))));
            }
            _ => {}
        }
    }
    for (index, block) in graph.blocks.iter_mut().enumerate() {
        let old = std::mem::take(&mut block.items);
        let mut buckets = vec![Vec::new(); old.len() + 1];
        for (position, item) in std::mem::take(&mut moves[index]) {
            buckets[position].push(item);
        }
        for (position, item) in old.into_iter().enumerate() {
            block.items.append(&mut buckets[position]);
            if !matches!(item, BlockItem::Effect(id) if remove[id.0]) {
                block.items.push(item);
            }
        }
        block.items.append(buckets.last_mut().unwrap());
    }
}

fn can_sink(
    graph: &FunctionGraph,
    summaries: &[Effects],
    read: EffectId,
    target: BlockId,
    predecessors: &[Vec<usize>],
) -> bool {
    let origin = graph.effects[read.0].origin;
    let mut block = target;
    let mut visited = HashSet::new();
    while block != origin {
        if !visited.insert(block) || predecessors[block.0].len() != 1 {
            return false;
        }
        block = BlockId(predecessors[block.0][0]);
        if block == origin {
            break;
        }
        if graph.blocks[block.0]
            .items
            .iter()
            .any(|item| blocks_read(graph, summaries, read, *item))
        {
            return false;
        }
    }
    let items = &graph.blocks[origin.0].items;
    let position = items
        .iter()
        .position(|item| *item == BlockItem::Effect(read))
        .unwrap();
    !items[position + 1..]
        .iter()
        .any(|item| blocks_read(graph, summaries, read, *item))
}

fn blocks_read(
    graph: &FunctionGraph,
    summaries: &[Effects],
    read: EffectId,
    item: BlockItem,
) -> bool {
    let BlockItem::Effect(other) = item else {
        return false;
    };
    let source = &graph.effects[read.0].operation;
    let other = &graph.effects[other.0].operation;
    effects::blocks_read(other, source, graph, summaries)
}

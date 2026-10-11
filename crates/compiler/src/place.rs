//! Place calculations directly in their function graph.
//! Dominance owns availability; effects retain their authored snapshot ordering.
use crate::{body::*, Expression, Program};
use std::collections::HashMap;
mod analysis;
mod availability;
mod demand;
mod dominance;
mod effects;
mod flow;
mod joins;
mod reads;
mod reduce;
#[cfg(test)]
mod tests;
mod value;
use analysis::{Assumption, ValueAnalysis};
use availability::{Availability, Checkpoint};
use dominance::Dominators;
use effects::Effects;
use flow::ReducedFlow;
use joins::Joins;
use reduce::Reducer;

/// Call summaries describe authored bodies, before placement removes dead work.
pub(super) struct Placement {
    summaries: Vec<Effects>,
}

impl Placement {
    pub(super) fn new(program: &Program) -> Self {
        Self {
            summaries: effects::infer(program),
        }
    }

    pub(super) fn place(&self, mut graph: FunctionGraph) -> FunctionGraph {
        // Release analysis and bindings before compacting the finished graph.
        place_calculations(&mut graph, &self.summaries);
        graph.compact(|operation| effects::observable(operation, &self.summaries));
        graph
    }
}

fn successors(graph: &FunctionGraph, reachable: &[bool]) -> Vec<Vec<usize>> {
    graph
        .blocks
        .iter()
        .enumerate()
        .map(|(id, _)| {
            if reachable[id] {
                graph
                    .outgoing(BlockId(id))
                    .into_iter()
                    .map(|edge| edge.target.0)
                    .collect()
            } else {
                Vec::new()
            }
        })
        .collect()
}
fn predecessors(graph: &FunctionGraph, reachable: &[bool]) -> Vec<Vec<usize>> {
    let mut predecessors = vec![Vec::new(); graph.blocks.len()];
    for (source, targets) in successors(graph, reachable).into_iter().enumerate() {
        for target in targets {
            if !predecessors[target].contains(&source) {
                predecessors[target].push(source);
            }
        }
    }
    predecessors
}

fn place_calculations(graph: &mut FunctionGraph, summaries: &[Effects]) {
    let reachable = graph.reachable();
    for (index, block) in graph.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for item in &block.items {
            let BlockItem::Effect(id) = item else {
                continue;
            };
            for memory in graph.effects[id.0]
                .operation
                .memories()
                .into_iter()
                .flatten()
            {
                if !graph.memories.contains(&memory) {
                    graph.memories.push(memory);
                }
            }
        }
    }
    // Remove unused result channels before they can create false read demands.
    graph.prune_unused(&reachable, |operation| {
        effects::observable(operation, summaries)
    });
    reads::prepare(graph, summaries, &reachable);
    let mut predecessors = predecessors(graph, &reachable);
    let mut dominators =
        Dominators::new(graph.entry.0, &successors(graph, &reachable), &predecessors);
    let flow = ReducedFlow::new(graph, &dominators, &predecessors);
    let reachable = graph.reachable();
    if flow.control_changed {
        predecessors = self::predecessors(graph, &reachable);
        dominators = Dominators::new(graph.entry.0, &successors(graph, &reachable), &predecessors);
    }
    let plan = demand::Plan::new(graph, &reachable, &dominators, &flow);
    let mut children = vec![Vec::new(); graph.blocks.len()];
    for (block, parent) in dominators.parent.iter().enumerate() {
        if let Some(parent) = parent {
            if *parent != block {
                children[*parent].push(block);
            }
        }
    }
    // Joins can be allocated before their arms. Visit acyclic predecessors
    // first so a join can retain facts and reuse computed incoming values.
    for children in &mut children {
        children.sort_by_key(|&block| dominators.rank[block]);
    }
    let joins = Joins::new(graph, &predecessors, dominators);
    let mut placer = Placer {
        graph,
        reachable,
        flow,
        available: Availability::default(),
        plan,
        joins,
    };
    enum Visit {
        Enter(usize),
        Leave {
            block: usize,
            checkpoint: Checkpoint,
        },
    }
    let mut work = vec![Visit::Enter(placer.graph.entry.0)];
    while let Some(visit) = work.pop() {
        match visit {
            Visit::Enter(index) => {
                if !placer.reachable[index] {
                    continue;
                }
                let checkpoint = placer.available.checkpoint();
                placer.joins.prepare(
                    placer.graph,
                    &placer.reachable,
                    index,
                    &mut placer.available,
                );
                placer.block(BlockId(index));
                placer
                    .joins
                    .record(index, placer.available.bindings_since(checkpoint));
                work.push(Visit::Leave {
                    block: index,
                    checkpoint,
                });
                work.extend(children[index].iter().rev().copied().map(Visit::Enter));
            }
            Visit::Leave { block, checkpoint } => {
                placer.available.restore(checkpoint);
                placer
                    .joins
                    .complete(block, placer.flow.analysis(BlockId(block)));
            }
        }
    }
}

struct Placer<'a> {
    graph: &'a mut FunctionGraph,
    reachable: Vec<bool>,
    flow: ReducedFlow,
    available: Availability,
    plan: demand::Plan,
    joins: Joins,
}
impl Placer<'_> {
    fn block(&mut self, block: BlockId) {
        let items = std::mem::take(&mut self.graph.blocks[block.0].items);
        for item in items {
            let BlockItem::Effect(id) = item else {
                panic!("construction only places effects");
            };
            let operation = self.graph.effects[id.0]
                .operation
                .clone()
                .map_inputs(|value| self.materialize(value, block));
            self.graph.effects[id.0].operation = operation;
            self.graph.blocks[block.0].items.push(BlockItem::Effect(id));
            for result in self.graph.effects[id.0].results.clone() {
                self.available.bind(result, result);
            }
        }
        for value in std::mem::take(&mut self.plan.shared[block.0]) {
            self.materialize(value, block);
        }
        let previous_targets: Vec<_> = self
            .graph
            .outgoing(block)
            .into_iter()
            .map(|edge| edge.target)
            .collect();
        let mut exit = self.graph.blocks[block.0].exit.clone();
        exit.map_inputs(|value| self.materialize(value, block));
        self.graph.blocks[block.0].exit = exit;
        if self
            .graph
            .outgoing(block)
            .into_iter()
            .map(|edge| edge.target)
            .ne(previous_targets)
        {
            // Folding only removes edges, so existing dominance stays valid.
            // Placement and joins must stop using paths those edges kept alive.
            self.reachable = self.graph.reachable();
        }
    }
}

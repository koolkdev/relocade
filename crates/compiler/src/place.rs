//! Place calculations directly in their function graph.
//! Dominance owns availability; effects retain their authored snapshot ordering.
use crate::{body::*, Expression, FunctionKind, Program};
use std::collections::HashMap;
mod availability;
mod coverage;
mod demand;
mod dominance;
mod effects;
mod facts;
mod joins;
mod reads;
mod reuse;
mod shared;
mod specialize;
#[cfg(test)]
mod tests;
mod value;
use availability::{Availability, Checkpoint};
use dominance::Dominators;
use effects::Effects;
use facts::ScalarFacts;
use joins::Joins;
use specialize::{BlockScope, Specializer};

pub(super) fn module(program: &mut Program) -> Vec<(usize, FunctionGraph)> {
    let summaries = effects::infer(program);
    program
        .functions
        .iter_mut()
        .enumerate()
        .filter_map(|(id, function)| {
            let FunctionKind::Defined(body) = &mut function.kind else {
                return None;
            };
            let mut graph = body
                .take()
                .expect("defined function completed construction");
            place(&mut graph, &summaries);
            Some((id, graph))
        })
        .collect()
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

fn place(graph: &mut FunctionGraph, summaries: &[Effects]) {
    // Release placement's facts and bindings before pruning the finished graph.
    let placement = place_calculations(graph, summaries);
    graph.compact(|operation| effects::observable(operation, summaries));
    if placement.has_copies {
        if let Some(replacements) = reuse::share(graph, placement.dominators) {
            graph.replace_values(replacements);
        }
    }
}

struct Placement {
    has_copies: bool,
    // Reusable only when specialization left the control-flow edges unchanged.
    dominators: Option<Dominators>,
}

fn place_calculations(graph: &mut FunctionGraph, summaries: &[Effects]) -> Placement {
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
    let predecessors = predecessors(graph, &reachable);
    let dominators = Dominators::new(graph.entry.0, &successors(graph, &reachable), &predecessors);
    let shared = demand::schedules(graph, &reachable, &dominators);
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
        specializer: Specializer::default(),
        available: Availability::default(),
        shared,
        joins,
        changed_edges: false,
    };
    enum Visit {
        Enter(usize),
        Leave {
            block: usize,
            checkpoint: Checkpoint,
            scope: BlockScope,
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
                let incoming = placer.joins.prepare(
                    placer.graph,
                    &placer.reachable,
                    index,
                    &mut placer.available,
                );
                let scope = placer.specializer.begin_block(incoming);
                // A unique predecessor's selected edge supplies facts valid on
                // every entrance. Dominator ancestry preserves them afterwards.
                if predecessors[index].len() == 1 {
                    let source = predecessors[index][0];
                    match placer.graph.blocks[source].exit.clone() {
                        Exit::If {
                            condition,
                            taken,
                            otherwise,
                        } if taken.target != otherwise.target => {
                            placer.specializer.assume(
                                &placer.graph.values,
                                &placer.available,
                                condition,
                                taken.target.0 == index,
                            );
                        }
                        Exit::Switch {
                            selector,
                            cases,
                            default,
                        } if default.target.0 != index => {
                            let keys: Vec<_> = cases
                                .iter()
                                .filter(|(_, edge)| edge.target.0 == index)
                                .map(|(key, _)| *key)
                                .collect();
                            if keys.len() == 1 {
                                placer.specializer.equal(
                                    &placer.graph.values,
                                    &placer.available,
                                    selector,
                                    u64::from(keys[0]),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                placer.block(BlockId(index));
                placer
                    .joins
                    .record(index, placer.available.bindings_since(checkpoint));
                work.push(Visit::Leave {
                    block: index,
                    checkpoint,
                    scope,
                });
                work.extend(children[index].iter().rev().copied().map(Visit::Enter));
            }
            Visit::Leave {
                block,
                checkpoint,
                scope,
            } => {
                placer.available.restore(checkpoint);
                placer.joins.complete(block, placer.specializer.facts());
                placer.specializer.end_block(scope);
            }
        }
    }
    let has_copies = placer.available.has_copies;
    Placement {
        has_copies,
        dominators: (has_copies && !placer.changed_edges).then(|| placer.joins.into_dominators()),
    }
}

struct Placer<'a> {
    graph: &'a mut FunctionGraph,
    reachable: Vec<bool>,
    specializer: Specializer,
    available: Availability,
    shared: Vec<Vec<usize>>,
    joins: Joins,
    changed_edges: bool,
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
        self.materialize_shared(block);
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
            // Folding only removes edges, so the old tree remains conservative.
            // Placement and joins must stop using paths those edges kept alive.
            // Sharing rebuilds the tree to find newly available placements.
            self.reachable = self.graph.reachable();
            self.changed_edges = true;
        }
    }
}

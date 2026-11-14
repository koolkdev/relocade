//! Compile explicit definitions, then lower their surviving calculations.
use crate::{body::*, BuiltProgram, Expression};

mod control;
mod dominance;
mod effects;
mod facts;
mod finish;
mod form;
mod lower;
mod numbering;
mod reads;
mod simplify;
mod versions;

use dominance::Dominators;
use effects::Effects;

pub(super) struct Optimizer {
    summaries: Vec<Effects>,
}

impl Optimizer {
    pub(super) fn new(program: &BuiltProgram) -> Self {
        Self {
            summaries: effects::infer(program),
        }
    }

    pub(super) fn optimize(&self, mut graph: crate::body::FunctionGraph) -> SsaGraph {
        let mut reachable = graph.reachable();
        for (index, block) in graph.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            for &item in &block.items {
                if let BlockItem::Effect(id) = item {
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
        }
        graph.prune_unused(&reachable, |op| effects::observable(op, &self.summaries));
        reads::prepare(&mut graph, &self.summaries, &reachable);
        let mut incoming = predecessors(&graph, &reachable);
        let mut dominators =
            Dominators::new(graph.entry.0, &successors(&graph, &reachable), &incoming);
        if control::fold_repeated(&mut graph, &dominators, &incoming) {
            reachable = graph.reachable();
            incoming = predecessors(&graph, &reachable);
            dominators = Dominators::new(graph.entry.0, &successors(&graph, &reachable), &incoming);
        }
        let mut graph = form::build(graph, &reachable, &dominators);
        let mut changed = simplify::run(&mut graph, &dominators, &incoming);
        let reachable = graph.reachable();
        graph.prune_unused(&reachable, |op| effects::observable(op, &self.summaries));
        lower::run(&mut graph, &dominators, &mut changed);
        finish::run(&mut graph, &changed);
        graph.compact(|op| effects::observable(op, &self.summaries));
        graph
    }
}

fn successors(
    graph: &crate::body::FunctionGraph<impl Phase>,
    reachable: &[bool],
) -> Vec<Vec<usize>> {
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

fn predecessors(
    graph: &crate::body::FunctionGraph<impl Phase>,
    reachable: &[bool],
) -> Vec<Vec<usize>> {
    let mut result = vec![Vec::new(); graph.blocks.len()];
    for (source, targets) in successors(graph, reachable).into_iter().enumerate() {
        for target in targets {
            if !result[target].contains(&source) {
                result[target].push(source);
            }
        }
    }
    result
}

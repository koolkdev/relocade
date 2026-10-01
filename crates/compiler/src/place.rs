//! Place calculations directly in their function graph.
//! Dominance owns availability; effects retain their authored snapshot ordering.
use crate::{body::*, Expression, FunctionKind, Program};
use std::collections::HashMap;
mod demand;
mod dominance;
mod effects;
mod facts;
mod joins;
mod reads;
mod shared;
mod specialize;
mod value;
use dominance::Dominators;
use effects::Effects;
use facts::Facts;
use joins::Joins;
use specialize::Specializer;

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
    let reachable = graph.reachable();
    for (index, block) in graph.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for item in &block.items {
            let BlockItem::Effect(id) = item else {
                continue;
            };
            if let Some(location) = graph.effects[id.0].operation.location() {
                if !graph.memories.contains(&location.memory) {
                    graph.memories.push(location.memory);
                }
            }
        }
    }
    // Remove unused result channels before they can create false read demands.
    remove_unused(graph, summaries);
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
    // first so a join can reuse values already computed on its incoming edges.
    for children in &mut children {
        children.sort_by_key(|&block| dominators.rank[block]);
    }
    let original_exits: Vec<_> = graph
        .blocks
        .iter()
        .map(|block| block.exit.clone())
        .collect();
    let joins = Joins::new(graph, &predecessors, dominators);
    let mut placer = Placer {
        graph,
        specializer: Specializer::default(),
        available: HashMap::new(),
        available_log: Vec::new(),
        expressions: HashMap::new(),
        expression_log: Vec::new(),
        shared,
        joins,
    };
    enum Visit {
        Enter(usize),
        Leave {
            block: usize,
            values: usize,
            expressions: usize,
            facts: Facts,
        },
    }
    let mut work = vec![Visit::Enter(placer.graph.entry.0)];
    while let Some(visit) = work.pop() {
        match visit {
            Visit::Enter(index) => {
                let saved = placer.specializer.begin_block();
                let values = placer.available_log.len();
                let expressions = placer.expression_log.len();
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
                            placer.specializer.facts_mut().assume(
                                &placer.graph.values,
                                condition,
                                taken.target.0 == index,
                            );
                            if let Exit::If {
                                condition: original,
                                ..
                            } = original_exits[source]
                            {
                                placer.specializer.facts_mut().assume(
                                    &placer.graph.values,
                                    original,
                                    taken.target.0 == index,
                                );
                            }
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
                                placer.specializer.facts_mut().equal(
                                    &placer.graph.values,
                                    selector,
                                    u64::from(keys[0]),
                                );
                                if let Exit::Switch {
                                    selector: original, ..
                                } = original_exits[source]
                                {
                                    placer.specializer.facts_mut().equal(
                                        &placer.graph.values,
                                        original,
                                        u64::from(keys[0]),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
                placer.joins.prepare(placer.graph, index, &placer.available);
                placer.block(BlockId(index));
                placer.joins.record(
                    index,
                    placer.available_log[values..]
                        .iter()
                        .filter_map(|&(id, previous)| {
                            let value = placer.available[&id];
                            (previous != Some(value)).then_some((id, value))
                        })
                        .chain(
                            placer
                                .specializer
                                .residuals()
                                .filter_map(|(&id, residual)| {
                                    (!placer.available.contains_key(&id))
                                        .then(|| {
                                            placer.available.get(residual).map(|&value| (id, value))
                                        })
                                        .flatten()
                                }),
                        ),
                );
                work.push(Visit::Leave {
                    block: index,
                    values,
                    expressions,
                    facts: saved,
                });
                work.extend(children[index].iter().rev().copied().map(Visit::Enter));
            }
            Visit::Leave {
                block,
                values,
                expressions,
                facts,
            } => {
                while placer.available_log.len() > values {
                    let (id, previous) = placer.available_log.pop().unwrap();
                    if let Some(previous) = previous {
                        placer.available.insert(id, previous);
                    } else {
                        placer.available.remove(&id);
                    }
                }
                while placer.expression_log.len() > expressions {
                    let (key, previous) = placer.expression_log.pop().unwrap();
                    if let Some(previous) = previous {
                        placer.expressions.insert(key, previous);
                    } else {
                        placer.expressions.remove(&key);
                    }
                }
                let completed = std::mem::replace(placer.specializer.facts_mut(), facts);
                placer.joins.complete(block, completed);
            }
        }
    }
    remove_unused(placer.graph, summaries);
}

struct Placer<'a> {
    graph: &'a mut FunctionGraph,
    specializer: Specializer,
    // A recipe and its executed value while that execution dominates the cursor.
    available: HashMap<usize, usize>,
    available_log: Vec<(usize, Option<usize>)>,
    // Canonical operations on placed inputs available in the same dominance scope.
    expressions: HashMap<Value, usize>,
    expression_log: Vec<(Value, Option<usize>)>,
    shared: Vec<Vec<usize>>,
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
                self.define(result, result);
            }
        }
        self.materialize_shared(block);
        let mut exit = self.graph.blocks[block.0].exit.clone();
        exit.map_inputs(|value| self.materialize(value, block));
        self.graph.blocks[block.0].exit = exit;
    }
}

fn remove_unused(graph: &mut FunctionGraph, summaries: &[Effects]) {
    let reachable = graph.reachable();
    let mut live_values = vec![false; graph.values.len()];
    let mut live_effects = vec![false; graph.effects.len()];
    let mut incoming = vec![Vec::new(); graph.values.len()];
    let mut pending = Vec::new();
    for (id, block) in graph.blocks.iter().enumerate() {
        if !reachable[id] {
            continue;
        }
        match &block.exit {
            Exit::Return(values)
            | Exit::TailCall {
                arguments: values, ..
            } => pending.extend(values),
            Exit::If { condition, .. } => pending.push(*condition),
            Exit::Switch { selector, .. } => pending.push(*selector),
            _ => {}
        }
        for edge in graph.outgoing(BlockId(id)) {
            for (&parameter, &argument) in graph.blocks[edge.target.0]
                .parameters
                .iter()
                .zip(&edge.arguments)
            {
                incoming[parameter].push(argument);
            }
        }
        for item in &block.items {
            if let BlockItem::Effect(effect) = item {
                if reads::observable(&graph.effects[effect.0].operation, summaries) {
                    live_effects[effect.0] = true;
                    pending.extend(graph.inputs(*item));
                }
            }
        }
    }
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut live_values[id], true) {
            continue;
        }
        if let Some(producer) = graph.producer_of(id) {
            if let BlockItem::Effect(effect) = producer {
                if std::mem::replace(&mut live_effects[effect.0], true) {
                    continue;
                }
            }
            pending.extend(graph.inputs(producer));
        } else if matches!(
            graph.values[id].definition,
            ValueDefinition::Parameter { .. }
        ) {
            pending.extend(incoming[id].iter().copied());
        }
    }
    let keep: Vec<Vec<bool>> = graph
        .blocks
        .iter()
        .enumerate()
        .map(|(id, block)| {
            block
                .parameters
                .iter()
                .map(|&parameter| id == graph.entry.0 || live_values[parameter])
                .collect()
        })
        .collect();
    for block in &mut graph.blocks {
        block.items.retain(|item| match item {
            BlockItem::Evaluate(id) => live_values[*id],
            BlockItem::Effect(id) => live_effects[id.0],
        });
        let trim = |edge: &mut Edge| {
            let mut index = 0;
            edge.arguments.retain(|_| {
                let keep = keep[edge.target.0][index];
                index += 1;
                keep
            });
        };
        for edge in block.exit.edges_mut() {
            trim(edge);
        }
    }
    for (id, block) in graph.blocks.iter_mut().enumerate() {
        if id == graph.entry.0 {
            continue;
        }
        block.parameters.retain(|&parameter| live_values[parameter]);
        for (component, &parameter) in block.parameters.iter().enumerate() {
            graph.values.values[parameter].definition = ValueDefinition::Parameter {
                block: BlockId(id),
                component,
            };
        }
    }
}

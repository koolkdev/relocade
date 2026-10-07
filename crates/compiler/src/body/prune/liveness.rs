//! Remove unneeded executions and parameters before reclaiming graph storage.
use super::super::{BlockId, BlockItem, Edge, Exit, FunctionGraph, Operation, ValueDefinition};

/// Liveness of referenced values and required operation executions.
/// Compaction also retains unused results belonging to a live producer.
pub(super) struct Retained {
    pub(super) values: Vec<bool>,
    pub(super) effects: Vec<bool>,
}

pub(super) fn prune(
    graph: &mut FunctionGraph,
    reachable: &[bool],
    observable: impl Fn(&Operation) -> bool,
) -> Retained {
    let mut live_values = vec![false; graph.values.len()];
    let mut live_calculations = vec![false; graph.values.len()];
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
                if observable(&graph.effects[effect.0].operation) {
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
            let live = match producer {
                BlockItem::Evaluate(root) => &mut live_calculations[root],
                BlockItem::Effect(effect) => &mut live_effects[effect.0],
            };
            if std::mem::replace(live, true) {
                continue;
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
            BlockItem::Evaluate(id) => live_calculations[*id],
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
    Retained {
        values: live_values,
        effects: live_effects,
    }
}

//! Dependency order for placement, independent of expression allocation order.

use super::Tree;
use crate::body::{Body, Operation, Target, ValueDefinition};

pub(super) fn values(body: &Body, tree: &Tree<'_>) -> Vec<usize> {
    let mut visited = vec![false; body.values.len()];
    let mut order = Vec::with_capacity(body.values.len());
    let mut pending = Vec::new();
    for root in 0..body.values.len() {
        pending.push((root, false));
        while let Some((id, ready)) = pending.pop() {
            if visited[id] {
                continue;
            }
            let definition = body.values[id].definition;
            let operation = match definition {
                ValueDefinition::Load { site }
                | ValueDefinition::JoinResult { site, .. }
                | ValueDefinition::OperationResult { site, .. } => tree.0.operation(site),
                _ => None,
            };
            if ready {
                if let Some(Operation::Call { outputs, .. }) = operation {
                    // All results of an invocation receive demand before its
                    // arguments. Keep the group together even if only one is used.
                    for &output in outputs {
                        visited[output] = true;
                        order.push(output);
                    }
                } else {
                    visited[id] = true;
                    order.push(id);
                }
                continue;
            }
            pending.push((id, true));
            match (definition, operation) {
                (ValueDefinition::Expression(expression), _) => {
                    pending.extend(expression.inputs().map(|&input| (input, false)));
                }
                (_, Some(Operation::Load { location })) => {
                    pending.push((location.base, false));
                }
                (_, Some(Operation::Call { invocation, .. })) => {
                    pending.extend(invocation.arguments.iter().map(|&input| (input, false)));
                }
                (ValueDefinition::JoinResult { site, component }, Some(operation)) => {
                    for arm in operation.children() {
                        for (_, arguments) in arm.exits_to(Target::exit(site)) {
                            pending.push((arguments[component], false));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    order
}

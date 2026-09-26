//! Dependency order for placement, independent of expression allocation order.

use super::Tree;
use crate::{control::Target, Body, Operation, ValueKind};

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
            let kind = body.values[id].kind;
            let operation = match kind {
                ValueKind::JoinResult { site, .. } | ValueKind::OperationResult { site, .. } => {
                    tree.0.operation(site)
                }
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
            pending.extend(kind.inputs().map(|input| (input, false)));
            match (kind, operation) {
                (_, Some(Operation::Call { invocation, .. })) => {
                    pending.extend(invocation.arguments.iter().map(|&input| (input, false)));
                }
                (ValueKind::JoinResult { site, component }, Some(operation)) => {
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

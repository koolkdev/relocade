//! Simplifies completed control blocks before effects and value placement.
use super::Block;
use crate::{Operation, Value, ValueDefinition};

impl Block {
    pub(crate) fn fold_constants(&mut self, values: &[Value]) {
        for operation in &mut self.operations {
            match operation {
                Operation::Block { block, .. }
                | Operation::Loop { block, .. }
                | Operation::BranchIf { taken: block, .. } => block.fold_constants(values),
                Operation::If {
                    branch,
                    else_branch,
                    ..
                } => {
                    branch.fold_constants(values);
                    if let Some(other) = else_branch {
                        other.fold_constants(values);
                    }
                }
                Operation::Switch { cases, default, .. } => {
                    for case in cases {
                        case.block.fold_constants(values);
                    }
                    default.fold_constants(values);
                }
                _ => {}
            }
            let condition = match operation {
                Operation::If { condition, .. } | Operation::BranchIf { condition, .. } => {
                    *condition
                }
                _ => continue,
            };
            let ValueDefinition::Constant(bits) = values[condition].definition else {
                continue;
            };
            // Construction has checked every arm and enclosing result join. Keep
            // this slot: values and labels refer to their original operation sites.
            *operation = match std::mem::replace(operation, Operation::Nop) {
                Operation::If {
                    branch,
                    else_branch,
                    outputs,
                    ..
                } => {
                    let block = if bits != 0 { Some(branch) } else { else_branch };
                    match block {
                        Some(block) => Operation::Block { block, outputs },
                        None => Operation::Nop,
                    }
                }
                Operation::BranchIf { taken, .. } if bits != 0 => Operation::Block {
                    block: taken,
                    outputs: Vec::new(),
                },
                Operation::BranchIf { .. } => Operation::Nop,
                _ => unreachable!("only conditional operations have a condition"),
            };
        }
    }
}

//! Simplifies completed control blocks before effects and value placement.
use crate::body::{Block, Operation, Value, ValueDefinition};

pub(super) fn control(block: &mut Block, values: &[Value]) {
    for operation in &mut block.operations {
        match operation {
            Operation::Block { block, .. }
            | Operation::Loop { block, .. }
            | Operation::BranchIf { taken: block, .. } => control(block, values),
            Operation::If {
                branch,
                else_branch,
                ..
            } => {
                control(branch, values);
                if let Some(other) = else_branch {
                    control(other, values);
                }
            }
            Operation::Switch { cases, default, .. } => {
                for case in cases {
                    control(&mut case.block, values);
                }
                control(default, values);
            }
            _ => {}
        }
        let condition = match operation {
            Operation::If { condition, .. } | Operation::BranchIf { condition, .. } => *condition,
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

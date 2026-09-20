//! Simplifies completed control regions before effects and value placement.
use super::Region;
use crate::{Operation, Value, ValueKind};

impl Region {
    pub(crate) fn fold_constants(&mut self, values: &[Value]) {
        for operation in &mut self.operations {
            match operation {
                Operation::Block { region, .. }
                | Operation::Loop { region, .. }
                | Operation::BranchIf { taken: region, .. } => region.fold_constants(values),
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
                        case.region.fold_constants(values);
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
            let ValueKind::Constant(bits) = values[condition].kind else {
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
                    let region = if bits != 0 { Some(branch) } else { else_branch };
                    match region {
                        Some(region) => Operation::Block { region, outputs },
                        None => Operation::Nop,
                    }
                }
                Operation::BranchIf { taken, .. } if bits != 0 => Operation::Block {
                    region: taken,
                    outputs: Vec::new(),
                },
                Operation::BranchIf { .. } => Operation::Nop,
                _ => unreachable!("only conditional operations have a condition"),
            };
        }
    }
}

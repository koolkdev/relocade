//! Share small, nontrapping calculations when their inputs already dominate both uses.
use super::*;
use crate::expression::{BinaryOp, CompareOp, TernaryOp, UnaryOp};

impl Reuse<'_> {
    pub(super) fn hoist(&mut self, number: usize, block: BlockId) -> Option<usize> {
        let executions = self.executions[number].as_slice();
        for &(_, value) in executions.iter().rev() {
            let value = self.resolve(value);
            let Some(result) = self.graph.values.expression(value) else {
                continue;
            };
            if result.component != 0 || !self.graph.values[value].ty.is_integer() {
                continue;
            }
            let expression = result.expression.map(|&input| self.resolve(input));
            let cheap = matches!(
                expression,
                Expression::Unary {
                    operator: UnaryOp::Convert
                        | UnaryOp::LowBits { .. }
                        | UnaryOp::ZeroTest { .. }
                        | UnaryOp::SignExtend { .. }
                        | UnaryOp::BitCount { .. }
                        | UnaryOp::Reinterpret,
                    ..
                } | Expression::Binary {
                    operator: BinaryOp::Bitwise(_)
                        | BinaryOp::Compare(CompareOp::Integer(_))
                        | BinaryOp::Integer(
                            crate::integer::BinaryOp::Add | crate::integer::BinaryOp::Sub
                        ),
                    ..
                } | Expression::Shift { .. }
                    | Expression::Ternary {
                        operator: TernaryOp::Select,
                        ..
                    }
            );
            if !cheap {
                continue;
            }
            let owner = self.graph.values.definition_block(value)?;
            let common = self.dominators.common(owner.0, block.0)?;
            if common == owner.0 {
                // Keep a dominating definition before its existing users.
                return Some(value);
            }
            if !expression.inputs().all(|&input| {
                matches!(
                    self.graph.values[input].definition,
                    ValueDefinition::Literal(_)
                ) || self
                    .graph
                    .values
                    .definition_block(input)
                    .is_some_and(|owner| self.dominators.dominates(owner.0, common))
            }) {
                continue;
            }
            self.graph
                .move_calculation(result.producer, BlockId(common));
            self.graph.set_expression(result.producer, expression);
            self.executions[number].push((BlockId(common), value));
            return Some(value);
        }
        None
    }
}

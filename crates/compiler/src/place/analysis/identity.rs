//! Prove operand identities for the carrier positions a consumer observes.

use super::ValueAnalysis;
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    Expression,
};

impl ValueAnalysis {
    pub(in crate::place) fn identity(
        &self,
        table: &ValueTable,
        id: usize,
        observed: u128,
    ) -> Option<usize> {
        match table[id].definition {
            ValueDefinition::Expression(Expression::Binary {
                operator: Binary::Bitwise(operator),
                left,
                right,
            }) if operator != BitwiseOp::Xor || observed != u128::MAX => {
                if table[id].ty.is_scalar() {
                    let mask = observed as u64 & table[id].ty.carrier().mask();
                    let a = self.scalar_carrier_bits(table, left);
                    let b = self.scalar_carrier_bits(table, right);
                    if a.absorbs(operator, b, mask) {
                        Some(left)
                    } else if b.absorbs(operator, a, mask) {
                        Some(right)
                    } else {
                        None
                    }
                } else {
                    let a = self.vector_bits(table, left);
                    let b = self.vector_bits(table, right);
                    if a.absorbs(operator, b, observed) {
                        Some(left)
                    } else if b.absorbs(operator, a, observed) {
                        Some(right)
                    } else {
                        None
                    }
                }
            }
            ValueDefinition::Expression(Expression::Binary {
                operator: Binary::ReplaceLane { lane_type, lane },
                left: vector,
                right: value,
            }) => {
                let replaced = u128::from(lane_type.mask()) << (lane * lane_type.bits());
                if observed & replaced == 0 {
                    return Some(vector);
                }
                let ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::ExtractLane { lane: extracted },
                    input,
                }) = table[value].definition
                else {
                    return None;
                };
                if lane != extracted || lane_type != table[value].ty {
                    return None;
                }
                if vector == input || observed & !replaced == 0 {
                    return Some(input);
                }
                let common = self
                    .vector_bits(table, vector)
                    .common(self.vector_bits(table, input));
                (observed & !replaced & !common.mask == 0).then_some(input)
            }
            ValueDefinition::Expression(Expression::Unary {
                operator: Unary::LowBits { bits },
                input,
            }) if table[input].ty == table[id].ty
                && observed & !u128::from(crate::integer::low_mask(bits)) == 0 =>
            {
                Some(input)
            }
            _ => None,
        }
    }
}

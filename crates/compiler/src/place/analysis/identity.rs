//! Prove identities for complete carriers, retaining logical lane widths.

use super::{Bits, ValueAnalysis};
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::low_mask,
    Expression,
};

impl ValueAnalysis {
    pub(in crate::place) fn identity(&self, table: &ValueTable, id: usize) -> Option<usize> {
        match table[id].definition {
            ValueDefinition::Expression(Expression::Binary {
                operator: Binary::Bitwise(operator),
                left,
                right,
            }) if matches!(operator, BitwiseOp::And | BitwiseOp::Or) => {
                if table[id].ty.is_scalar() {
                    let mask = table[id].ty.carrier().mask();
                    // Logical facts cannot prove upper carrier positions. Physical
                    // bounds can supply their zeros without narrowing the value.
                    let a = self.bits(table, left).union(Bits {
                        mask: mask & !low_mask(table.bounds[left].unsigned),
                        value: 0,
                    });
                    let b = self.bits(table, right).union(Bits {
                        mask: mask & !low_mask(table.bounds[right].unsigned),
                        value: 0,
                    });
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
                    if a.absorbs(operator, b, u128::MAX) {
                        Some(left)
                    } else if b.absorbs(operator, a, u128::MAX) {
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
                if vector == input {
                    return Some(input);
                }
                let replaced = u128::from(lane_type.mask()) << (lane * lane_type.bits());
                let common = self
                    .vector_bits(table, vector)
                    .common(self.vector_bits(table, input));
                (common.mask & !replaced == !replaced).then_some(input)
            }
            _ => None,
        }
    }
}

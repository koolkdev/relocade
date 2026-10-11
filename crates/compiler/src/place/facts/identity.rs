//! Operand identities proved for the complete encoding, including carrier bits.

use super::ValueFacts;
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    Expression,
};

impl ValueFacts {
    pub(in crate::place) fn identity(&self, table: &ValueTable, id: usize) -> Option<usize> {
        match table[id].definition {
            ValueDefinition::Expression(Expression::Bitwise {
                operator,
                left,
                right,
            }) if matches!(operator, BitwiseOp::And | BitwiseOp::Or) => {
                let mask = table[id].ty.carrier().bit_mask();
                // Logical facts leave upper scalar carrier bits unknown. Only
                // physical bounds can prove those bits zero.
                let a = self
                    .bits(table, left)
                    .with_zero_above(table.bounds[left].unsigned);
                let b = self
                    .bits(table, right)
                    .with_zero_above(table.bounds[right].unsigned);
                let left_zeros = a.mask & !a.value;
                let right_zeros = b.mask & !b.value;
                match operator {
                    BitwiseOp::Or if mask & !right_zeros & !a.value == 0 => Some(left),
                    BitwiseOp::Or if mask & !left_zeros & !b.value == 0 => Some(right),
                    BitwiseOp::And if mask & !b.value & !left_zeros == 0 => Some(left),
                    BitwiseOp::And if mask & !a.value & !right_zeros == 0 => Some(right),
                    _ => None,
                }
            }
            ValueDefinition::Expression(Expression::VectorReplace {
                vector,
                value,
                lane,
            }) => {
                let ValueDefinition::Expression(Expression::VectorExtract {
                    input,
                    lane: extracted,
                }) = table[value].definition
                else {
                    return None;
                };
                if lane != extracted {
                    return None;
                }
                let replaced = table[value].ty.bit_mask() << (lane * table[value].ty.bits());
                let same = self.bits(table, vector).common(self.bits(table, input));
                (same.mask & !replaced == !replaced).then_some(input)
            }
            _ => None,
        }
    }
}

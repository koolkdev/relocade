//! Operand identities proved for the observed encoding bits.

use super::ValueFacts;
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    Expression,
};

impl ValueFacts {
    pub(in crate::place) fn identity(
        &self,
        table: &ValueTable,
        id: usize,
        observed: u128,
    ) -> Option<usize> {
        match table[id].definition {
            ValueDefinition::Expression(Expression::Bitwise {
                operator,
                left,
                right,
            }) if matches!(operator, BitwiseOp::And | BitwiseOp::Or) => {
                let mask = observed & table[id].ty.carrier().bit_mask();
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
                let replaced = table[value].ty.bit_mask() << (lane * table[value].ty.bits());
                if observed & replaced == 0 {
                    return Some(vector);
                }
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
                let same = self.bits(table, vector).common(self.bits(table, input));
                (observed & !replaced & !same.mask == 0).then_some(input)
            }
            ValueDefinition::Expression(Expression::LowBits { input, bits })
                if table[input].ty == table[id].ty
                    && observed & !u128::from(crate::integer::low_mask(bits)) == 0 =>
            {
                Some(input)
            }
            _ => None,
        }
    }
}

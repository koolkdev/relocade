//! Admit logical expressions and choose their initial representation.
//!
//! Construction may discard bits outside the logical type. Refolding an existing
//! operation bypasses this entry and preserves every carrier bit. Both paths use
//! the same value table and algebraic folding helpers.

use super::Folder;
use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, UnaryOp as Unary,
};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::BitCountOp,
    Expression, Type,
};

pub(crate) fn build(
    values: &mut ValueTable,
    ty: Type,
    expression: Expression<usize>,
    component: usize,
) -> usize {
    Folder { values }.construct(ty, expression, component)
}

impl Folder<'_> {
    // Inputs carry logical types; canonical operations may retain wider physical bits.
    fn construct(&mut self, ty: Type, expression: Expression<usize>, component: usize) -> usize {
        let expression = expression.map(|&input| {
            let value = self.values[input];
            match value.definition {
                ValueDefinition::Literal(bits) => self.values.literal(value.ty, bits),
                _ => input,
            }
        });
        if let Some(literals) = self.literals(expression) {
            if let Some(bits) = literals.constant_result(ty, component) {
                return self.values.literal(ty, bits);
            }
        }
        if !ty.is_integer() {
            if let Some(source) = self.common_reinterpret_type(ty, expression) {
                let expression = expression.map(|&input| self.reinterpret(input, source));
                let result = self.construct(source, expression, component);
                return self.reinterpret(result, ty);
            }
        }
        match expression {
            Expression::Unary { operator, input } => match operator {
                Unary::SignExtend { input_type } => self.sign_extend(input, input_type, ty),
                Unary::BitCount {
                    operator,
                    input_type,
                } => self.bit_count(operator, input_type, input),
                Unary::ZeroTest { nonzero } => self.zero_test(input, nonzero),
                Unary::Convert => self.convert(input, ty),
                Unary::Reinterpret => self.reinterpret(input, ty),
                _ => self.fold(ty, expression),
            },
            Expression::Binary {
                operator,
                left,
                right,
            } => match operator {
                Binary::Integer(operator) => self.binary(operator, left, right),
                Binary::Bitwise(operator) => self.bitwise(operator, left, right),
                Binary::Compare(Compare::Integer(operator)) => self.compare(operator, left, right),
                _ => self.fold(ty, expression),
            },
            Expression::Shift {
                operator,
                value,
                count,
            } => match operator {
                Shift::Scalar(operator) => self.shift(operator, value, count),
                Shift::Rotate(operator) => self.rotate(operator, value, count),
                Shift::Lanes { .. } => self.fold(ty, expression),
            },
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => self.select(condition, when_true, when_false),
            Expression::MultiplyWide { .. } => self.fold_result(ty, expression, component),
        }
    }

    pub(super) fn bitwise(&mut self, operator: BitwiseOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        // Construction observes logical widths. Refolding an existing carrier
        // operation must retain its upper bits instead of repeating these rules.
        match (operator, a.scalar_literal(), b.scalar_literal()) {
            (BitwiseOp::And, _, Some(mask)) if mask == a.ty.mask() => left,
            (BitwiseOp::And, Some(mask), _) if mask == a.ty.mask() => right,
            (BitwiseOp::Or, _, Some(bits)) if bits == a.ty.mask() => right,
            (BitwiseOp::Or, Some(bits), _) if bits == a.ty.mask() => left,
            _ => self.fold(
                a.ty,
                Expression::Binary {
                    operator: Binary::Bitwise(operator),
                    left,
                    right,
                },
            ),
        }
    }

    fn bit_count(&mut self, operator: BitCountOp, input_type: Type, input: usize) -> usize {
        let input = self.fold(
            input_type,
            Expression::Unary {
                operator: Unary::LowBits {
                    bits: input_type.bits(),
                },
                input,
            },
        );
        self.fold(
            input_type,
            Expression::Unary {
                operator: Unary::BitCount {
                    operator,
                    input_type,
                },
                input,
            },
        )
    }
}

//! Shift and rotate folding, count conversion and logical-width lowering.

use super::Folder;
use crate::expression::{ShiftOp as Shift, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::ValueDefinition,
    integer::{self, BinaryOp, BitCountOp, RotateOp, ShiftOp},
    Expression, Type,
};

impl Folder<'_> {
    /// Moving the highest set bit to the top of its carrier preserves zero.
    /// Restrict the proof to full-width counts of the same physical input.
    pub(super) fn normalized_shift_source(&self, input: usize) -> Option<usize> {
        let ty = self.values[input].ty;
        if !matches!(ty, Type::I32 | Type::I64) {
            return None;
        }
        let ValueDefinition::Expression(Expression::Shift {
            operator: Shift::Scalar(ShiftOp::Left),
            value,
            mut count,
        }) = self.values[input].definition
        else {
            return None;
        };
        if self.values[value].ty != ty {
            return None;
        }
        while let ValueDefinition::Expression(Expression::Unary {
            operator: Unary::Convert,
            input: source,
        }) = self.values[count].definition
        {
            if self.values.bounds[source].unsigned > self.values[count].ty.carrier().bits() {
                return None;
            }
            count = source;
        }
        match self.values[count].definition {
            ValueDefinition::Expression(Expression::Unary {
                operator:
                    Unary::BitCount {
                        operator: BitCountOp::LeadingZeros,
                        input_type,
                    },
                input: source,
            }) if input_type == ty
                && self.values.representation(source) == self.values.representation(value) =>
            {
                Some(value)
            }
            _ => None,
        }
    }

    pub(super) fn fold_shift(&self, ty: Type, input: usize, count: usize) -> Option<usize> {
        let zero_count = matches!(self.values[count].scalar_literal(), Some(bits) if integer::shift_count(ty, bits as u32) == 0);
        (zero_count || self.values.bounds[input].unsigned == 0).then_some(input)
    }

    pub(super) fn fold_rotate(&self, ty: Type, input: usize, count: usize) -> Option<usize> {
        let identity_input = matches!(self.values[input].scalar_literal(), Some(bits) if bits == 0 || bits == ty.carrier().mask());
        self.fold_shift(ty, input, count)
            .or(identity_input.then_some(input))
    }

    pub(super) fn shift(&mut self, operator: ShiftOp, input: usize, count: usize) -> usize {
        let value = self.values[input];
        if let Some(input) = self.fold_shift(value.ty, input, count) {
            return input;
        }
        let input = match operator {
            ShiftOp::Left => input,
            ShiftOp::RightUnsigned => self.normalize(input),
            ShiftOp::RightSigned => self.sign_extend_carrier(input),
        };
        let count = if value.ty == Type::I64 {
            self.convert(count, Type::I64)
        } else {
            count
        };
        self.fold(
            value.ty,
            Expression::Shift {
                operator: Shift::Scalar(operator),
                value: input,
                count,
            },
        )
    }

    pub(super) fn rotate(&mut self, operator: RotateOp, input: usize, count: usize) -> usize {
        let value = self.values[input];
        if value.ty == Type::I1 {
            return input;
        }
        if matches!(value.ty, Type::I8 | Type::I16) {
            if matches!(self.values[count].scalar_literal(), Some(bits)
                if integer::rotate_count(value.ty, bits as u32) == 0)
                || matches!(value.scalar_literal(), Some(bits)
                    if bits == 0 || bits == value.ty.mask())
            {
                return input;
            }
            let input = self.normalize(input);
            let width = u64::from(value.ty.bits());
            let mask = self.values.literal(Type::I32, width - 1);
            let count = self.bitwise(BitwiseOp::And, count, mask);
            let width = self.values.literal(Type::I32, width);
            let remaining = self.binary(BinaryOp::Sub, width, count);
            let (left_count, right_count) = match operator {
                RotateOp::Left => (count, remaining),
                RotateOp::Right => (remaining, count),
            };
            // A zero count leaves the other term outside the logical low bits.
            let left = self.shift(ShiftOp::Left, input, left_count);
            let right = self.shift(ShiftOp::RightUnsigned, input, right_count);
            return self.bitwise(BitwiseOp::Or, left, right);
        }
        let count = if value.ty == Type::I64 {
            self.convert(count, Type::I64)
        } else {
            count
        };
        self.fold(
            value.ty,
            Expression::Shift {
                operator: Shift::Rotate(operator),
                value: input,
                count,
            },
        )
    }
}

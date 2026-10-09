//! Shift and rotate folding, count conversion and logical-width lowering.

use super::Folder;
use crate::{
    body::ValueDefinition,
    integer::{self, BinaryOp, RotateOp, ShiftOp},
    Expression, Type,
};

impl Folder<'_> {
    pub(super) fn fold_shift(&self, ty: Type, input: usize, count: usize) -> Option<usize> {
        let zero_count = matches!(self.values[count].definition,
            ValueDefinition::Literal(bits) if integer::shift_count(ty, bits as u32) == 0);
        (zero_count || self.values.bounds[input].unsigned == 0).then_some(input)
    }

    pub(super) fn fold_rotate(&self, ty: Type, input: usize, count: usize) -> Option<usize> {
        let identity_input = matches!(self.values[input].definition,
            ValueDefinition::Literal(bits) if bits == 0 || bits == ty.carrier().mask());
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
                operator,
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
            if matches!(self.values[count].definition, ValueDefinition::Literal(bits)
                if integer::rotate_count(value.ty, bits as u32) == 0)
                || matches!(value.definition, ValueDefinition::Literal(bits)
                    if bits == 0 || bits == value.ty.mask())
            {
                return input;
            }
            let input = self.normalize(input);
            let width = u64::from(value.ty.bits());
            let mask = self.values.literal(Type::I32, width - 1);
            let count = self.binary(BinaryOp::And, count, mask);
            let width = self.values.literal(Type::I32, width);
            let remaining = self.binary(BinaryOp::Sub, width, count);
            let (left_count, right_count) = match operator {
                RotateOp::Left => (count, remaining),
                RotateOp::Right => (remaining, count),
            };
            // A zero count leaves the other term outside the logical low bits.
            let left = self.shift(ShiftOp::Left, input, left_count);
            let right = self.shift(ShiftOp::RightUnsigned, input, right_count);
            return self.binary(BinaryOp::Or, left, right);
        }
        let count = if value.ty == Type::I64 {
            self.convert(count, Type::I64)
        } else {
            count
        };
        self.fold(
            value.ty,
            Expression::Rotate {
                operator,
                value: input,
                count,
            },
        )
    }
}

//! Shift and rotate folding, count conversion and logical-width lowering.

use super::ValueArena;
use crate::{
    body::{Value, ValueDefinition},
    integer::{self, BinaryOp, RotateOp, ShiftOp},
    Expression, Type,
};

impl ValueArena {
    pub(super) fn shift(&mut self, operator: ShiftOp, input: usize, count: usize) -> usize {
        let value = self.values[input];
        if let ValueDefinition::Constant(bits) = self.values[count].definition {
            let effective = integer::shift_count(value.ty, bits as u32);
            if effective == 0 {
                return input;
            }
            if let ValueDefinition::Constant(bits) = value.definition {
                let bits = integer::shift(value.ty, operator, bits, effective);
                return self.constant(value.ty, bits);
            }
        }
        if matches!(value.definition, ValueDefinition::Constant(0)) {
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
        self.intern(Value {
            ty: value.ty,
            definition: ValueDefinition::Expression(Expression::Shift {
                operator,
                value: input,
                count,
            }),
        })
    }

    pub(super) fn rotate(&mut self, operator: RotateOp, input: usize, count: usize) -> usize {
        let value = self.values[input];
        if let ValueDefinition::Constant(bits) = self.values[count].definition {
            let effective = integer::rotate_count(value.ty, bits as u32);
            if effective == 0 {
                return input;
            }
            if let ValueDefinition::Constant(bits) = value.definition {
                let bits = integer::rotate(value.ty, operator, bits, effective);
                return self.constant(value.ty, bits);
            }
        }
        if value.ty == Type::I1
            || matches!(value.definition, ValueDefinition::Constant(bits) if bits == 0 || bits == value.ty.mask())
        {
            return input;
        }
        if matches!(value.ty, Type::I8 | Type::I16) {
            let input = self.normalize(input);
            let width = u64::from(value.ty.bits());
            let mask = self.constant(Type::I32, width - 1);
            let count = self.binary(BinaryOp::And, count, mask);
            let width = self.constant(Type::I32, width);
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
        self.intern(Value {
            ty: value.ty,
            definition: ValueDefinition::Expression(Expression::Rotate {
                operator,
                value: input,
                count,
            }),
        })
    }
}

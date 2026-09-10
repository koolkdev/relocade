//! Shift and rotate folding, count conversion and logical-width lowering.

use super::{ExpressionArena, ValueArena};
use crate::{
    integer::{self, BinaryOp, RotateOp, ShiftOp},
    BuildError, Type, Value, ValueKind,
};

impl ExpressionArena {
    pub(crate) fn shift(
        &self,
        operator: ShiftOp,
        input: usize,
        count: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| arena.shift(operator, input, count))
    }

    pub(crate) fn rotate(
        &self,
        operator: RotateOp,
        input: usize,
        count: usize,
    ) -> Result<usize, BuildError> {
        self.with_open(|arena| arena.rotate(operator, input, count))
    }
}

impl ValueArena {
    fn shift(&mut self, operator: ShiftOp, input: usize, count: usize) -> usize {
        let value = self.values[input];
        if let ValueKind::Constant(bits) = self.values[count].kind {
            let effective = integer::shift_count(value.ty, bits as u32);
            if effective == 0 {
                return input;
            }
            if let ValueKind::Constant(bits) = value.kind {
                let bits = integer::shift(value.ty, operator, bits, effective);
                return self.constant(value.ty, bits);
            }
        }
        if matches!(value.kind, ValueKind::Constant(0)) {
            return input;
        }
        let input = match operator {
            ShiftOp::Left => input,
            ShiftOp::RightUnsigned => self.normalize(input),
            // Interpret the logical sign before shifting the Wasm carrier;
            // upper bits from narrow arithmetic need not be normalized.
            ShiftOp::RightSigned => self.sign_extend(
                input,
                if value.ty == Type::I64 {
                    Type::I64
                } else {
                    Type::I32
                },
            ),
        };
        let count = if value.ty == Type::I64 {
            self.convert(count, Type::I64)
        } else {
            count
        };
        self.intern(Value {
            ty: value.ty,
            kind: ValueKind::Shift {
                operator,
                value: input,
                count,
            },
        })
    }

    fn rotate(&mut self, operator: RotateOp, input: usize, count: usize) -> usize {
        let value = self.values[input];
        if let ValueKind::Constant(bits) = self.values[count].kind {
            let effective = integer::rotate_count(value.ty, bits as u32);
            if effective == 0 {
                return input;
            }
            if let ValueKind::Constant(bits) = value.kind {
                let bits = integer::rotate(value.ty, operator, bits, effective);
                return self.constant(value.ty, bits);
            }
        }
        if value.ty == Type::I1
            || matches!(value.kind, ValueKind::Constant(bits) if bits == 0 || bits == value.ty.mask())
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
            kind: ValueKind::Rotate {
                operator,
                value: input,
                count,
            },
        })
    }
}

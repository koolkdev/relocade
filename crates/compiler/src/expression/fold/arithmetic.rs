//! Arithmetic normalization, algebraic rewrites and modular constant offsets.

use super::Folder;
use crate::{
    body::ValueDefinition,
    integer::{self, BinaryOp, ShiftOp},
    Expression, Type,
};

impl Folder<'_> {
    pub(super) fn fold_multiply_wide(
        &mut self,
        signed: bool,
        left: usize,
        right: usize,
        component: usize,
    ) -> Option<usize> {
        let left_bounds = self.values.bounds[left];
        let right_bounds = self.values.bounds[right];
        let product_bits = if signed {
            left_bounds.signed + right_bounds.signed
        } else {
            left_bounds.unsigned + right_bounds.unsigned
        };
        // A product that fits one carrier has only zero or sign bits above it.
        // A factor restricted to zero or one also fits, even at full width.
        if product_bits > 64 && left_bounds.unsigned > 1 && right_bounds.unsigned > 1 {
            return None;
        }
        if component == 1 && !signed {
            return Some(self.values.literal(Type::I64, 0));
        }
        let low = self.fold(
            Type::I64,
            Expression::Binary {
                operator: BinaryOp::Mul,
                left,
                right,
            },
        );
        Some(if component == 0 {
            low
        } else {
            let count = self.values.literal(Type::I64, 63);
            self.fold(
                Type::I64,
                Expression::Shift {
                    operator: ShiftOp::RightSigned,
                    value: low,
                    count,
                },
            )
        })
    }

    // These rewrites preserve every result bit, so construction and path
    // specialization can use them without repeating logical normalization.
    pub(super) fn fold_binary(
        &mut self,
        ty: Type,
        operator: BinaryOp,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        let a = self.values.representation(left);
        let b = self.values.representation(right);
        let offset = match (
            operator,
            self.values[a].scalar_literal(),
            self.values[b].scalar_literal(),
        ) {
            (BinaryOp::Add, _, Some(offset)) => Some((left, offset)),
            (BinaryOp::Add, Some(offset), _) => Some((right, offset)),
            (BinaryOp::Sub, _, Some(offset)) => Some((left, 0_u64.wrapping_sub(offset))),
            _ => None,
        };
        if let Some((input, offset)) = offset {
            return Some(self.add_constant(ty, input, offset, ty.carrier().bits()));
        }
        match (
            operator,
            self.values[a].scalar_literal(),
            self.values[b].scalar_literal(),
        ) {
            (BinaryOp::DivUnsigned | BinaryOp::RemUnsigned, _, _)
                if ty == Type::I64
                    && self.values.bounds[left].unsigned <= 32
                    && self.values.bounds[right].unsigned <= 32 =>
            {
                // Both operands and the result fit 32 unsigned bits. The caller
                // restores the logical i64 type by zero-extending the result.
                let left = self.convert(left, Type::I32);
                let right = self.convert(right, Type::I32);
                Some(self.fold(
                    Type::I32,
                    Expression::Binary {
                        operator,
                        left,
                        right,
                    },
                ))
            }
            (BinaryOp::Mul, _, Some(1)) => Some(left),
            (BinaryOp::Mul, Some(1), _) => Some(right),
            (BinaryOp::Mul, _, Some(0)) | (BinaryOp::Mul, Some(0), _) => {
                Some(self.values.literal(ty, 0))
            }
            (BinaryOp::Sub, _, _) if a == b => Some(self.values.literal(ty, 0)),
            _ => None,
        }
    }

    pub(super) fn binary(&mut self, operator: BinaryOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        // These transformations interpret the logical width. Placement has
        // already chosen its carrier operations and must not repeat them.
        match (operator, a.scalar_literal(), b.scalar_literal()) {
            (BinaryOp::Add, _, Some(offset)) => self.add_constant(a.ty, left, offset, a.ty.bits()),
            (BinaryOp::Add, Some(offset), _) => self.add_constant(a.ty, right, offset, a.ty.bits()),
            (BinaryOp::Sub, _, Some(offset)) => {
                self.add_constant(a.ty, left, 0u64.wrapping_sub(offset), a.ty.bits())
            }
            _ => {
                let (left, right) = match operator {
                    BinaryOp::DivUnsigned | BinaryOp::RemUnsigned => {
                        (self.normalize(left), self.normalize(right))
                    }
                    BinaryOp::DivSigned | BinaryOp::RemSigned => (
                        self.sign_extend_carrier(left),
                        self.sign_extend_carrier(right),
                    ),
                    _ => (left, right),
                };
                self.fold(
                    a.ty,
                    Expression::Binary {
                        operator,
                        left,
                        right,
                    },
                )
            }
        }
    }

    pub(super) fn add_constant(
        &mut self,
        ty: Type,
        mut input: usize,
        mut offset: u64,
        observed_bits: u8,
    ) -> usize {
        // Combine only consecutive offsets. Conversions and other operations
        // remain boundaries. Construction observes the logical width; existing
        // carrier operations and explicit masks supply their own observed width.
        let mask = integer::low_mask(observed_bits);
        offset &= mask;
        while let Some((base, previous)) = self.constant_offset(input) {
            input = base;
            offset = offset.wrapping_add(previous) & mask;
        }
        if offset == 0 {
            return input;
        }
        if let Some(value) = self.values[input].scalar_literal() {
            return self
                .values
                .carrier_literal(ty, value.wrapping_add(offset) & mask);
        }
        let constant = self.values.carrier_literal(ty, offset);
        self.intern(
            ty,
            Expression::Binary {
                operator: BinaryOp::Add,
                left: input,
                right: constant,
            },
        )
    }

    pub(super) fn constant_offset(&self, input: usize) -> Option<(usize, u64)> {
        let ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Add,
            left,
            right,
        }) = self.values[input].definition
        else {
            return None;
        };
        self.values[right]
            .scalar_literal()
            .map(|offset| (left, offset))
    }
}

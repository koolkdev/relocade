//! Binary normalization, algebraic rewrites and modular constant offsets.

use super::Folder;
use crate::{
    body::ValueDefinition,
    integer::{self, BinaryOp},
    Expression, Type,
};

impl Folder<'_> {
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
        match (
            operator,
            self.values[a].definition,
            self.values[b].definition,
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
            (
                BinaryOp::Or | BinaryOp::Xor | BinaryOp::Add | BinaryOp::Sub,
                _,
                ValueDefinition::Constant(0),
            )
            | (BinaryOp::Mul, _, ValueDefinition::Constant(1)) => Some(left),
            (BinaryOp::Or | BinaryOp::Xor | BinaryOp::Add, ValueDefinition::Constant(0), _)
            | (BinaryOp::Mul, ValueDefinition::Constant(1), _) => Some(right),
            (BinaryOp::And | BinaryOp::Mul, _, ValueDefinition::Constant(0))
            | (BinaryOp::And | BinaryOp::Mul, ValueDefinition::Constant(0), _) => {
                Some(self.values.constant(ty, 0))
            }
            (BinaryOp::Sub | BinaryOp::Xor, _, _) if a == b => Some(self.values.constant(ty, 0)),
            (BinaryOp::And | BinaryOp::Or, _, _) => {
                if a == b {
                    return Some(left);
                }
                for (input, other) in [(left, b), (right, a)] {
                    if let ValueDefinition::Expression(Expression::Binary {
                        operator: nested,
                        left,
                        right,
                    }) = self.values[self.values.representation(input)].definition
                    {
                        if nested == operator
                            && (self.values.representation(left) == other
                                || self.values.representation(right) == other)
                        {
                            return Some(input);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub(super) fn binary(&mut self, operator: BinaryOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        if let (ValueDefinition::Constant(a), ValueDefinition::Constant(b)) =
            (a.definition, b.definition)
        {
            if let Some(bits) = integer::binary(self.values[left].ty, operator, a, b) {
                return self.values.constant(self.values[left].ty, bits);
            }
        }
        // These transformations interpret the logical width. Placement has
        // already chosen its carrier operations and must not repeat them.
        match (operator, a.definition, b.definition) {
            (BinaryOp::Add, _, ValueDefinition::Constant(offset)) => {
                self.add_constant(left, offset)
            }
            (BinaryOp::Add, ValueDefinition::Constant(offset), _) => {
                self.add_constant(right, offset)
            }
            (BinaryOp::Sub, _, ValueDefinition::Constant(offset)) => {
                self.add_constant(left, a.ty.normalize(0u64.wrapping_sub(offset)))
            }
            (BinaryOp::And, _, ValueDefinition::Constant(mask)) => self.and_constant(left, mask),
            (BinaryOp::And, ValueDefinition::Constant(mask), _) => self.and_constant(right, mask),
            (BinaryOp::Or, _, ValueDefinition::Constant(bits)) if bits == a.ty.mask() => right,
            (BinaryOp::Or, ValueDefinition::Constant(bits), _) if bits == a.ty.mask() => left,
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

    pub(super) fn add_constant(&mut self, mut input: usize, mut offset: u64) -> usize {
        let ty = self.values[input].ty;
        // Combine only consecutive offsets. Conversions and other operations
        // remain boundaries, and offsets wrap at the logical integer width.
        while let Some((base, previous)) = self.constant_offset(input) {
            input = base;
            offset = ty.normalize(offset.wrapping_add(previous));
        }
        if offset == 0 {
            return input;
        }
        let constant = self.values.constant(ty, offset);
        self.fold(
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
        match self.values[right].definition {
            ValueDefinition::Constant(offset) => Some((left, offset)),
            _ => None,
        }
    }

    fn and_constant(&mut self, input: usize, mask: u64) -> usize {
        let ty = self.values[input].ty;
        if mask == ty.mask() {
            return input;
        }
        let bits = mask.trailing_ones() as u8;
        if mask == integer::low_mask(bits) {
            return self.low_bits(input, bits);
        }
        if self.values.bounds[input].unsigned <= bits {
            return input;
        }
        let constant = self.values.constant(ty, mask);
        self.fold(
            ty,
            Expression::Binary {
                operator: BinaryOp::And,
                left: input,
                right: constant,
            },
        )
    }
}

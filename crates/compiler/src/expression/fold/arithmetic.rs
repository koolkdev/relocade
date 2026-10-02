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
            return Some(self.values.constant(Type::I64, 0));
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
            let count = self.values.constant(Type::I64, 63);
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
            self.values[a].definition,
            self.values[b].definition,
        ) {
            (BinaryOp::Add, _, ValueDefinition::Constant(offset)) => Some((left, offset)),
            (BinaryOp::Add, ValueDefinition::Constant(offset), _) => Some((right, offset)),
            (BinaryOp::Sub, _, ValueDefinition::Constant(offset)) => {
                Some((left, 0_u64.wrapping_sub(offset)))
            }
            _ => None,
        };
        if let Some((input, offset)) = offset {
            return Some(self.add_constant(ty, input, offset, ty.carrier().bits()));
        }
        if operator == BinaryOp::And {
            let masked = match (self.values[a].definition, self.values[b].definition) {
                (_, ValueDefinition::Constant(mask)) => self.and_constant(ty, left, mask),
                (ValueDefinition::Constant(mask), _) => self.and_constant(ty, right, mask),
                _ => None,
            };
            if masked.is_some() {
                return masked;
            }
        }
        if operator == BinaryOp::Or {
            if let Some(input) = self.rejoin_bits(ty, left, right) {
                return Some(input);
            }
        }
        if matches!(operator, BinaryOp::And | BinaryOp::Or | BinaryOp::Xor)
            && self.complementary_predicates(left, right)
        {
            return Some(
                self.values
                    .carrier_constant(ty, u64::from(operator != BinaryOp::And)),
            );
        }
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
            (BinaryOp::Or | BinaryOp::Xor, _, ValueDefinition::Constant(0))
            | (BinaryOp::Mul, _, ValueDefinition::Constant(1)) => Some(left),
            (BinaryOp::Or | BinaryOp::Xor, ValueDefinition::Constant(0), _)
            | (BinaryOp::Mul, ValueDefinition::Constant(1), _) => Some(right),
            (BinaryOp::Or, _, ValueDefinition::Constant(bits)) if bits == ty.carrier().mask() => {
                Some(right)
            }
            (BinaryOp::Or, ValueDefinition::Constant(bits), _) if bits == ty.carrier().mask() => {
                Some(left)
            }
            (BinaryOp::Mul, _, ValueDefinition::Constant(0))
            | (BinaryOp::Mul, ValueDefinition::Constant(0), _) => Some(self.values.constant(ty, 0)),
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
        // These transformations interpret the logical width. Placement has
        // already chosen its carrier operations and must not repeat them.
        match (operator, a.definition, b.definition) {
            (BinaryOp::Add, _, ValueDefinition::Constant(offset)) => {
                self.add_constant(a.ty, left, offset, a.ty.bits())
            }
            (BinaryOp::Add, ValueDefinition::Constant(offset), _) => {
                self.add_constant(a.ty, right, offset, a.ty.bits())
            }
            (BinaryOp::Sub, _, ValueDefinition::Constant(offset)) => {
                self.add_constant(a.ty, left, 0u64.wrapping_sub(offset), a.ty.bits())
            }
            (BinaryOp::And, _, ValueDefinition::Constant(mask)) if mask == a.ty.mask() => left,
            (BinaryOp::And, ValueDefinition::Constant(mask), _) if mask == a.ty.mask() => right,
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
        if let ValueDefinition::Constant(value) = self.values[input].definition {
            return self
                .values
                .carrier_constant(ty, value.wrapping_add(offset) & mask);
        }
        let constant = self.values.carrier_constant(ty, offset);
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
        match self.values[right].definition {
            ValueDefinition::Constant(offset) => Some((left, offset)),
            _ => None,
        }
    }

    fn and_constant(&mut self, ty: Type, input: usize, mask: u64) -> Option<usize> {
        let bits = mask.trailing_ones() as u8;
        if self.values.bounds[input].unsigned <= bits {
            return Some(input);
        }
        if mask == integer::low_mask(bits) {
            return Some(self.fold_low_bits(ty, input, bits));
        }
        None
    }
}

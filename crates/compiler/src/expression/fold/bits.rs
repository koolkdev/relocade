//! Logical width changes and the physical bits carried between observations.
use super::Folder;
use crate::expression::{BinaryOp as Binary, ShiftOp as Shift, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::ValueDefinition,
    integer::{self, ShiftOp},
    Expression, Type,
};

struct MaskedBits {
    input: usize,
    mask: u64,
}

impl Folder<'_> {
    /// Rejoining bits extracted from the same carrier only needs their union
    /// mask. Match explicit masks and restored shifts, never logical type widths.
    pub(super) fn rejoin_bits(&mut self, ty: Type, left: usize, right: usize) -> Option<usize> {
        let left = self.restored_bits(left);
        let right = self.restored_bits(right);
        if left.input != right.input {
            return None;
        }
        let mask = self.values.carrier_literal(ty, left.mask | right.mask);
        Some(self.fold(
            ty,
            Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::And),
                left: left.input,
                right: mask,
            },
        ))
    }

    fn masked_bits(&self, input: usize) -> MaskedBits {
        let input = self.values.representation(input);
        let (input, mask) = match self.values[input].definition {
            ValueDefinition::Expression(Expression::Unary {
                operator: Unary::LowBits { bits },
                input,
            }) => (input, integer::low_mask(bits)),
            ValueDefinition::Expression(Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::And),
                left,
                right,
            }) => match (
                self.values[left].scalar_literal(),
                self.values[right].scalar_literal(),
            ) {
                (_, Some(mask)) => (left, mask),
                (Some(mask), _) => (right, mask),
                _ => (input, self.values[input].ty.carrier().mask()),
            },
            _ => (input, self.values[input].ty.carrier().mask()),
        };
        MaskedBits {
            input: self.values.representation(input),
            mask,
        }
    }

    fn restored_bits(&self, input: usize) -> MaskedBits {
        let mut bits = self.masked_bits(input);
        let ValueDefinition::Expression(Expression::Shift {
            operator: Shift::Scalar(ShiftOp::Left),
            value,
            count,
        }) = self.values[bits.input].definition
        else {
            return bits;
        };
        let carrier = self.values[bits.input].ty.carrier();
        let Some(count) = self.values[count].scalar_literal() else {
            return bits;
        };
        let count = integer::shift_count(carrier, count as u32);
        let shifted = self.masked_bits(value);
        let ValueDefinition::Expression(Expression::Shift {
            operator: Shift::Scalar(ShiftOp::RightUnsigned),
            value,
            count: reverse,
        }) = self.values[shifted.input].definition
        else {
            return bits;
        };
        let Some(reverse) = self.values[reverse].scalar_literal() else {
            return bits;
        };
        if self.values[shifted.input].ty.carrier() != carrier
            || integer::shift_count(carrier, reverse as u32) != count
        {
            return bits;
        }
        let source = self.masked_bits(value);
        bits.input = source.input;
        bits.mask &= (shifted.mask << count) & source.mask & carrier.mask();
        bits
    }

    /// The explicit mask defines the observed bits, including after operands
    /// have been replaced by branch facts. Intermediate type views keep their
    /// carrier bits; this fold does not normalize them to their logical widths.
    pub(super) fn fold_low_bits(&mut self, ty: Type, input: usize, bits: u8) -> usize {
        debug_assert!(bits <= ty.carrier().bits());
        if bits == 0 {
            return self.values.literal(ty, 0);
        }
        if self.values.bounds[input].unsigned <= bits {
            return input;
        }
        let mask = integer::low_mask(bits);
        let mut base = input;
        let mut offset = 0_u64;
        loop {
            if let Some(value) = self.values[base].scalar_literal() {
                return self
                    .values
                    .carrier_literal(ty, value.wrapping_add(offset) & mask);
            }
            match self.values[base].definition {
                ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::LowBits { bits: kept },
                    input,
                }) if kept >= bits => base = input,
                ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::Convert,
                    input,
                }) if self.values[input].ty.bits() >= bits
                    && self.values[base].ty.carrier() == self.values[input].ty.carrier() =>
                {
                    base = input;
                }
                ValueDefinition::Expression(Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::And),
                    left,
                    right,
                }) => {
                    let (input, kept) = match (
                        self.values[left].scalar_literal(),
                        self.values[right].scalar_literal(),
                    ) {
                        (_, Some(mask)) => (left, mask.trailing_ones()),
                        (Some(mask), _) => (right, mask.trailing_ones()),
                        _ => break,
                    };
                    if kept < u32::from(bits) {
                        break;
                    }
                    base = input;
                }
                ValueDefinition::Expression(Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::Or | BitwiseOp::Xor),
                    left,
                    right,
                }) => {
                    // Disjoint masked bits cannot affect the low result,
                    // including after an accumulated modular offset.
                    base = if self.masked_bits(left).mask & mask == 0 {
                        right
                    } else if self.masked_bits(right).mask & mask == 0 {
                        left
                    } else {
                        break;
                    };
                }
                _ => {
                    // Carries above the observed width are irrelevant. Offset
                    // arithmetic must itself retain every observed bit.
                    if self.values[base].ty.bits() < bits {
                        break;
                    }
                    let Some((input, addend)) = self.constant_offset(base) else {
                        break;
                    };
                    base = input;
                    offset = offset.wrapping_add(addend) & mask;
                }
            }
        }
        let base = if self.values[base].ty == ty {
            base
        } else {
            self.fold(
                ty,
                Expression::Unary {
                    operator: Unary::Convert,
                    input: base,
                },
            )
        };
        let input = self.add_constant(ty, base, offset, bits);
        if self.values.bounds[input].unsigned <= bits {
            return input;
        }
        self.intern(
            ty,
            Expression::Unary {
                operator: Unary::LowBits { bits },
                input,
            },
        )
    }

    pub(super) fn convert(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        let input = if source.ty.bits() < target.bits() {
            self.normalize(input)
        } else {
            input
        };
        self.fold(
            target,
            Expression::Unary {
                operator: Unary::Convert,
                input,
            },
        )
    }

    pub(super) fn sign_extend(&mut self, input: usize, input_type: Type, target: Type) -> usize {
        if input_type == target {
            if self.values[input].ty == target {
                return input;
            }
            return self.fold(
                target,
                Expression::Unary {
                    operator: Unary::Convert,
                    input,
                },
            );
        }
        self.fold(
            target,
            Expression::Unary {
                operator: Unary::SignExtend { input_type },
                input,
            },
        )
    }

    pub(super) fn fold_sign_extend(
        &mut self,
        target: Type,
        input_type: Type,
        input: usize,
    ) -> Option<usize> {
        let canonical = self.values.bounds[input].signed <= input_type.bits();
        if canonical && target != Type::I64 {
            // Preserve the existing signed representation and its sharing when
            // only the logical type widens; unsigned convert() would mask it.
            return Some(input);
        }
        if canonical && input_type != Type::I32 {
            let alias = self.fold(
                Type::I32,
                Expression::Unary {
                    operator: Unary::Convert,
                    input,
                },
            );
            // Crossing into i64 still needs the signed carrier extension.
            return Some(self.fold(
                target,
                Expression::Unary {
                    operator: Unary::SignExtend {
                        input_type: Type::I32,
                    },
                    input: alias,
                },
            ));
        }
        None
    }

    pub(super) fn sign_extend_carrier(&mut self, input: usize) -> usize {
        // Interpret the logical sign before a signed carrier operation; narrow
        // arithmetic can leave upper bits that do not belong to the value.
        let carrier = self.values[input].ty.carrier();
        self.sign_extend(input, self.values[input].ty, carrier)
    }

    pub(crate) fn normalize(&mut self, input: usize) -> usize {
        let ty = self.values[input].ty;
        if ty == Type::V64 {
            if self.values.bounds[input].unsigned <= 64 {
                return input;
            }
            let mask = self.values.literal(ty, u64::MAX);
            return self.fold(
                ty,
                Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::And),
                    left: input,
                    right: mask,
                },
            );
        }
        if !ty.is_integer() {
            return input;
        }
        self.fold(
            ty,
            Expression::Unary {
                operator: Unary::LowBits { bits: ty.bits() },
                input,
            },
        )
    }
}

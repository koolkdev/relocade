//! Logical width changes and the physical bits carried between observations.
use super::Folder;
use crate::{
    body::ValueDefinition,
    integer::{self, BinaryOp, ShiftOp},
    Expression, Type,
};

struct MaskedBits {
    input: usize,
    mask: u64,
}

impl Folder<'_> {
    /// XOR cancels repeated operands and combines adjacent constant masks.
    /// Inputs already identify their carrier representations.
    pub(super) fn fold_xor(&mut self, ty: Type, left: usize, right: usize) -> Option<usize> {
        for (nested, other) in [(left, right), (right, left)] {
            let ValueDefinition::Expression(Expression::Binary {
                operator: BinaryOp::Xor,
                left: inner_left,
                right: inner_right,
            }) = self.values[nested].definition
            else {
                continue;
            };
            let a = self.values.representation(inner_left);
            let b = self.values.representation(inner_right);
            if b == other {
                return Some(inner_left);
            }
            if a == other {
                return Some(inner_right);
            }
            let ValueDefinition::Constant(outer_mask) = self.values[other].definition else {
                continue;
            };
            let (base, inner_mask) = match (self.values[a].definition, self.values[b].definition) {
                (_, ValueDefinition::Constant(mask)) => (inner_left, mask),
                (ValueDefinition::Constant(mask), _) => (inner_right, mask),
                _ => continue,
            };
            let mask = self.values.carrier_constant(ty, inner_mask ^ outer_mask);
            return Some(self.fold(
                ty,
                Expression::Binary {
                    operator: BinaryOp::Xor,
                    left: base,
                    right: mask,
                },
            ));
        }
        None
    }

    /// Rejoining bits extracted from the same carrier only needs their union
    /// mask. Match explicit masks and restored shifts, never logical type widths.
    pub(super) fn rejoin_bits(&mut self, ty: Type, left: usize, right: usize) -> Option<usize> {
        let left = self.restored_bits(left);
        let right = self.restored_bits(right);
        if left.input != right.input {
            return None;
        }
        let mask = self.values.carrier_constant(ty, left.mask | right.mask);
        Some(self.fold(
            ty,
            Expression::Binary {
                operator: BinaryOp::And,
                left: left.input,
                right: mask,
            },
        ))
    }

    fn masked_bits(&self, input: usize) -> MaskedBits {
        let input = self.values.representation(input);
        let (input, mask) = match self.values[input].definition {
            ValueDefinition::Expression(Expression::LowBits { input, bits }) => {
                (input, integer::low_mask(bits))
            }
            ValueDefinition::Expression(Expression::Binary {
                operator: BinaryOp::And,
                left,
                right,
            }) => match (self.values[left].definition, self.values[right].definition) {
                (_, ValueDefinition::Constant(mask)) => (left, mask),
                (ValueDefinition::Constant(mask), _) => (right, mask),
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
            operator: ShiftOp::Left,
            value,
            count,
        }) = self.values[bits.input].definition
        else {
            return bits;
        };
        let carrier = self.values[bits.input].ty.carrier();
        let ValueDefinition::Constant(count) = self.values[count].definition else {
            return bits;
        };
        let count = integer::shift_count(carrier, count as u32);
        let shifted = self.masked_bits(value);
        let ValueDefinition::Expression(Expression::Shift {
            operator: ShiftOp::RightUnsigned,
            value,
            count: reverse,
        }) = self.values[shifted.input].definition
        else {
            return bits;
        };
        let ValueDefinition::Constant(reverse) = self.values[reverse].definition else {
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
            return self.values.constant(ty, 0);
        }
        if self.values.bounds[input].unsigned <= bits {
            return input;
        }
        let mask = integer::low_mask(bits);
        let mut base = input;
        let mut offset = 0_u64;
        loop {
            match self.values[base].definition {
                ValueDefinition::Constant(value) => {
                    return self
                        .values
                        .carrier_constant(ty, value.wrapping_add(offset) & mask);
                }
                ValueDefinition::Expression(Expression::LowBits { input, bits: kept })
                    if kept >= bits =>
                {
                    base = input
                }
                ValueDefinition::Expression(Expression::Convert { input })
                    if self.values[input].ty.bits() >= bits
                        && self.values[base].ty.carrier() == self.values[input].ty.carrier() =>
                {
                    base = input;
                }
                ValueDefinition::Expression(Expression::Binary {
                    operator: BinaryOp::And,
                    left,
                    right,
                }) => {
                    let (input, kept) =
                        match (self.values[left].definition, self.values[right].definition) {
                            (_, ValueDefinition::Constant(mask)) => (left, mask.trailing_ones()),
                            (ValueDefinition::Constant(mask), _) => (right, mask.trailing_ones()),
                            _ => break,
                        };
                    if kept < u32::from(bits) {
                        break;
                    }
                    base = input;
                }
                ValueDefinition::Expression(Expression::Binary {
                    operator: BinaryOp::Or | BinaryOp::Xor,
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
            self.fold(ty, Expression::Convert { input: base })
        };
        let input = self.add_constant(ty, base, offset, bits);
        if self.values.bounds[input].unsigned <= bits {
            return input;
        }
        self.intern(ty, Expression::LowBits { input, bits })
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
        self.fold(target, Expression::Convert { input })
    }

    pub(super) fn sign_extend(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        self.fold(target, Expression::SignExtend { input })
    }

    pub(super) fn fold_sign_extend(&mut self, target: Type, input: usize) -> Option<usize> {
        let source = self.values[input];
        let canonical = self.values.bounds[input].signed <= source.ty.bits();
        if canonical && target != Type::I64 {
            // Preserve the existing signed representation and its sharing when
            // only the logical type widens; unsigned convert() would mask it.
            return Some(input);
        }
        if canonical && source.ty != Type::I32 {
            let alias = self.fold(Type::I32, Expression::Convert { input });
            // Crossing into i64 still needs the signed carrier extension.
            return Some(self.fold(target, Expression::SignExtend { input: alias }));
        }
        None
    }

    pub(super) fn sign_extend_carrier(&mut self, input: usize) -> usize {
        // Interpret the logical sign before a signed carrier operation; narrow
        // arithmetic can leave upper bits that do not belong to the value.
        let carrier = self.values[input].ty.carrier();
        self.sign_extend(input, carrier)
    }

    pub(crate) fn normalize(&mut self, input: usize) -> usize {
        let ty = self.values[input].ty;
        if !ty.is_integer() {
            return input;
        }
        self.fold(
            ty,
            Expression::LowBits {
                input,
                bits: ty.bits(),
            },
        )
    }
}

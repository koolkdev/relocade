//! Logical width changes and the physical bits carried between observations.
use super::Folder;
use crate::{
    body::ValueDefinition,
    integer::{self, BinaryOp},
    Expression, Type,
};

impl Folder<'_> {
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
        self.fold(
            ty,
            Expression::LowBits {
                input,
                bits: ty.bits(),
            },
        )
    }
}

//! Shared bitwise identities and integer mask normalization.

use super::Folder;
use crate::{bitwise::BitwiseOp, body::ValueDefinition, integer, Expression, Type};

impl Folder<'_> {
    // These rewrites preserve every result bit, including unobserved upper bits.
    pub(super) fn fold_bitwise(
        &mut self,
        ty: Type,
        operator: BitwiseOp,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        let a = self.values.representation(left);
        let b = self.values.representation(right);
        if operator == BitwiseOp::And {
            let masked = match (
                self.values[a].scalar_literal(),
                self.values[b].scalar_literal(),
            ) {
                (_, Some(mask)) => self.and_constant(ty, left, mask),
                (Some(mask), _) => self.and_constant(ty, right, mask),
                _ => None,
            };
            if masked.is_some() {
                return masked;
            }
        }
        if operator == BitwiseOp::Or {
            if let Some(input) = self.rejoin_bits(ty, left, right) {
                return Some(input);
            }
        }
        if self.complementary_predicates(left, right) {
            return Some(
                self.values
                    .carrier_literal(ty, u64::from(operator != BitwiseOp::And)),
            );
        }
        match (
            operator,
            self.values[a].scalar_literal(),
            self.values[b].scalar_literal(),
        ) {
            (BitwiseOp::Or | BitwiseOp::Xor, _, Some(0)) => Some(left),
            (BitwiseOp::Or | BitwiseOp::Xor, Some(0), _) => Some(right),
            (BitwiseOp::Or, _, Some(bits)) if bits == ty.carrier().mask() => Some(right),
            (BitwiseOp::Or, Some(bits), _) if bits == ty.carrier().mask() => Some(left),
            (BitwiseOp::Xor, _, _) if a == b => Some(self.values.literal(ty, 0)),
            (BitwiseOp::Xor, _, _) => self.fold_xor(ty, a, b),
            (BitwiseOp::And | BitwiseOp::Or, _, _) => {
                if a == b {
                    return Some(left);
                }
                for (input, other) in [(left, b), (right, a)] {
                    if let ValueDefinition::Expression(Expression::Bitwise {
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
        }
    }

    pub(super) fn bitwise(&mut self, operator: BitwiseOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        // Construction observes logical widths. Refolding an existing carrier
        // operation must retain its upper bits instead of repeating these rules.
        match (operator, a.scalar_literal(), b.scalar_literal()) {
            (BitwiseOp::And, _, Some(mask)) if mask == a.ty.mask() => left,
            (BitwiseOp::And, Some(mask), _) if mask == a.ty.mask() => right,
            (BitwiseOp::Or, _, Some(bits)) if bits == a.ty.mask() => right,
            (BitwiseOp::Or, Some(bits), _) if bits == a.ty.mask() => left,
            _ => self.fold(
                a.ty,
                Expression::Bitwise {
                    operator,
                    left,
                    right,
                },
            ),
        }
    }

    /// XOR cancels repeated operands and combines adjacent constant masks.
    /// Inputs already identify their carrier representations.
    fn fold_xor(&mut self, ty: Type, left: usize, right: usize) -> Option<usize> {
        for (nested, other) in [(left, right), (right, left)] {
            let ValueDefinition::Expression(Expression::Bitwise {
                operator: BitwiseOp::Xor,
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
            let Some(outer_mask) = self.values[other].scalar_literal() else {
                continue;
            };
            let (base, inner_mask) = match (
                self.values[a].scalar_literal(),
                self.values[b].scalar_literal(),
            ) {
                (_, Some(mask)) => (inner_left, mask),
                (Some(mask), _) => (inner_right, mask),
                _ => continue,
            };
            let mask = self.values.carrier_literal(ty, inner_mask ^ outer_mask);
            return Some(self.fold(
                ty,
                Expression::Bitwise {
                    operator: BitwiseOp::Xor,
                    left: base,
                    right: mask,
                },
            ));
        }
        None
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

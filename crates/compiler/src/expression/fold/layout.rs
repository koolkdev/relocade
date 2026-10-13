//! Known byte and bit selections recover immediate lane operations.

use super::*;
use crate::expression::{BinaryOp as Binary, ShuffleMask, UnaryOp as Unary};

impl Folder<'_> {
    pub(super) fn fold_shuffle_bytes(
        &mut self,
        ty: Type,
        input_type: Type,
        left: usize,
        right: usize,
        indices: usize,
    ) -> Option<usize> {
        let indices = self.vector_literal(indices)?.to_le_bytes();
        let width = input_type.bits() / 8;
        let has_left = indices.iter().any(|&index| index < width);
        let has_right = indices
            .iter()
            .any(|&index| (width..width * 2).contains(&index));
        let has_zero = indices.iter().any(|&index| index >= width * 2);
        if !has_left && !has_right {
            return Some(self.values.literal(ty, 0));
        }
        let same = self.values.representation(left) == self.values.representation(right);
        let (left, right, mask) = if !has_left || !has_right || same {
            let input = if has_left { left } else { right };
            let other = if has_zero {
                self.values.literal(Type::V128, 0)
            } else {
                input
            };
            (
                input,
                other,
                indices.map(|index| {
                    if index < width * 2 {
                        index % width
                    } else {
                        16 | (index & 15)
                    }
                }),
            )
        } else {
            (
                left,
                right,
                indices.map(|index| {
                    if index < width {
                        index
                    } else if index < width * 2 {
                        16 + index - width
                    } else {
                        0
                    }
                }),
            )
        };
        let result = self.fold(
            ty,
            Expression::Binary {
                operator: Binary::ShuffleImmediate(ShuffleMask::new(mask)),
                left,
                right,
            },
        );
        // An immediate shuffle names two inputs. A third source of zero bytes
        // needs an explicit mask, which placement can share like any other value.
        if has_zero && has_left && has_right && !same {
            let mask =
                u128::from_le_bytes(indices.map(|index| if index < width * 2 { 255 } else { 0 }));
            let mask = self.values.literal(Type::V128, mask);
            Some(self.fold(
                ty,
                Expression::Binary {
                    operator: Binary::Bitwise(crate::bitwise::BitwiseOp::And),
                    left: result,
                    right: mask,
                },
            ))
        } else {
            Some(result)
        }
    }

    pub(super) fn fold_swizzle(
        &mut self,
        ty: Type,
        input_type: Type,
        input: usize,
        indices: usize,
    ) -> Option<usize> {
        if self.vector_literal(input) == Some(0) {
            return Some(input);
        }
        let indices = self.vector_literal(indices)?.to_le_bytes();
        let width = input_type.bits() / 8;
        if indices.iter().all(|&index| index >= width) {
            return Some(self.values.literal(Type::V128, 0));
        }
        if width == 16 && indices == std::array::from_fn(|i| i as u8) {
            return Some(input);
        }
        let right = if indices.iter().any(|&index| index >= width) {
            self.values.literal(Type::V128, 0)
        } else {
            input
        };
        let indices = indices.map(|index| {
            if index < width {
                index
            } else {
                16 | (index & 15)
            }
        });
        Some(self.fold(
            ty,
            Expression::Binary {
                operator: Binary::ShuffleImmediate(ShuffleMask::new(indices)),
                left: input,
                right,
            },
        ))
    }

    pub(super) fn fold_select_bits(
        &mut self,
        ty: Type,
        mask: usize,
        when_true: usize,
        when_false: usize,
    ) -> Option<usize> {
        if when_true == when_false {
            return Some(when_true);
        }
        let mask = self.vector_literal(mask)?;
        match mask {
            0 => return Some(when_false),
            u128::MAX => return Some(when_true),
            _ => {}
        }
        let offset = mask.trailing_zeros() as u8;
        let (input, lane_type) = match self.values[when_true].definition {
            ValueDefinition::Expression(Expression::Unary {
                operator: Unary::Splat { lane_type },
                input,
            }) => (input, lane_type),
            ValueDefinition::Literal(bits) => {
                let lane_type = match (mask >> offset).trailing_ones() {
                    8 => Type::I8,
                    16 => Type::I16,
                    32 => Type::I32,
                    64 => Type::I64,
                    _ => return None,
                };
                (
                    self.values
                        .literal(lane_type, (u128::from(bits) >> offset) as u64),
                    lane_type,
                )
            }
            _ => return None,
        };
        if !offset.is_multiple_of(lane_type.bits())
            || mask != u128::from(lane_type.mask()) << offset
        {
            return None;
        }
        Some(self.fold(
            ty,
            Expression::Binary {
                operator: Binary::ReplaceLane {
                    lane_type,
                    lane: offset / lane_type.bits(),
                },
                left: when_false,
                right: input,
            },
        ))
    }
}

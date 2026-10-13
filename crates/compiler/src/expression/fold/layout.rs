//! Known byte and bit selections recover immediate lane operations.

use super::*;
use crate::expression::{BinaryOp as Binary, ShuffleMask, UnaryOp as Unary};

impl Folder<'_> {
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

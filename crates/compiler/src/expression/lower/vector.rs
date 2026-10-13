//! Byte routing keeps logical source widths until native SIMD selection.

use super::{fold, Binary, BitwiseOp, Expression, Type, ValueTable};
use crate::expression::{LaneBinaryOp, ShuffleMask, TernaryOp as Ternary};

pub(super) fn lower(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    match expression {
        Expression::Binary {
            operator: Binary::Swizzle {
                input_type: Type::V64,
            },
            left,
            right,
        } => {
            let mask = values.literal(Type::V128, u128::from(u64::MAX));
            let left = fold(
                values,
                Type::V128,
                Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::And),
                    left,
                    right: mask,
                },
            );
            swizzle(values, ty, left, right)
        }
        Expression::Ternary {
            operator: Ternary::Shuffle {
                input_type: Type::V64,
            },
            first: left,
            second: right,
            third: indices,
        } => {
            let mask = ShuffleMask::new(std::array::from_fn(|byte| {
                if byte < 8 {
                    byte as u8
                } else {
                    byte as u8 + 8
                }
            }));
            let joined = fold(
                values,
                Type::V128,
                Expression::Binary {
                    operator: Binary::ShuffleImmediate(mask),
                    left,
                    right,
                },
            );
            swizzle(values, ty, joined, indices)
        }
        Expression::Ternary {
            operator: Ternary::Shuffle {
                input_type: Type::V128,
            },
            first: left,
            second: right,
            third: indices,
        } => {
            let left = swizzle(values, ty, left, indices);
            let offset = values.literal(Type::V128, u128::from_le_bytes([16; 16]));
            let indices = fold(
                values,
                Type::V128,
                Expression::Binary {
                    operator: Binary::Vector {
                        operator: LaneBinaryOp::Sub,
                        lane_type: Type::I8,
                    },
                    left: indices,
                    right: offset,
                },
            );
            let right = swizzle(values, ty, right, indices);
            fold(
                values,
                ty,
                Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::Or),
                    left,
                    right,
                },
            )
        }
        _ => unreachable!("only semantic byte routing needs vector lowering"),
    }
}

fn swizzle(values: &mut ValueTable, ty: Type, left: usize, indices: usize) -> usize {
    fold(
        values,
        ty,
        Expression::Binary {
            operator: Binary::Swizzle {
                input_type: Type::V128,
            },
            left,
            right: indices,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::ValueDefinition;

    #[test]
    fn lowering_preserves_every_carrier_byte_at_both_logical_widths() {
        for ty in [Type::V64, Type::V128] {
            let width = ty.bits() / 8;
            for group in 0..16_u8 {
                let mut values = ValueTable::default();
                let left = values.carrier_literal(
                    ty,
                    u128::from_le_bytes(std::array::from_fn(|i| 0x40 + i as u8)),
                );
                let right = values.carrier_literal(
                    ty,
                    u128::from_le_bytes(std::array::from_fn(|i| 0x80 + i as u8)),
                );
                let selectors: [u8; 16] = std::array::from_fn(|i| group * 16 + i as u8);
                let indices = values.carrier_literal(ty, u128::from_le_bytes(selectors));
                let result = lower(
                    &mut values,
                    ty,
                    Expression::Ternary {
                        operator: Ternary::Shuffle { input_type: ty },
                        first: left,
                        second: right,
                        third: indices,
                    },
                );
                let expected = selectors.map(|index| {
                    if index < width {
                        0x40 + index
                    } else if index < 2 * width {
                        0x80 + index - width
                    } else {
                        0
                    }
                });
                let ValueDefinition::Literal(bits) = values[result].definition else {
                    panic!("literal primitives fold");
                };
                assert_eq!(u128::from(bits), u128::from_le_bytes(expected));
            }
        }
    }
}

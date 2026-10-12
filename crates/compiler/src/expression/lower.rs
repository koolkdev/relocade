//! Expand compound operations into ordinary, carrier-preserving expressions.
use super::{BinaryOp as Binary, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::{BinaryOp, BitCountOp},
    Expression, Type,
};

impl<V> Expression<V> {
    pub(crate) fn needs_lowering(&self) -> bool {
        matches!(
            self,
            Self::Unary {
                operator: Unary::BitCount {
                    operator: BitCountOp::LeadingZeros | BitCountOp::TrailingZeros,
                    input_type: Type::I1 | Type::I8 | Type::I16,
                },
                ..
            }
        )
    }
}

pub(crate) fn lower(values: &mut ValueTable, id: usize) -> usize {
    let value = values[id];
    let ValueDefinition::Expression(expression) = value.definition else {
        return id;
    };
    if !expression.needs_lowering() {
        return id;
    }
    let Expression::Unary {
        operator: Unary::BitCount {
            operator,
            input_type,
        },
        input,
    } = expression
    else {
        unreachable!()
    };
    // Construction already normalized the input. The sentinel and padding
    // belong to its i32 carrier, not to the narrower logical input type.
    let input = fold(
        values,
        Type::I32,
        Expression::Unary {
            operator: Unary::Convert,
            input,
        },
    );
    let input = if operator == BitCountOp::TrailingZeros {
        let sentinel = values.literal(Type::I32, 1_u64 << input_type.bits());
        fold(
            values,
            Type::I32,
            Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::Or),
                left: input,
                right: sentinel,
            },
        )
    } else {
        input
    };
    let count = fold(
        values,
        Type::I32,
        Expression::Unary {
            operator: Unary::BitCount {
                operator,
                input_type: Type::I32,
            },
            input,
        },
    );
    let result = if operator == BitCountOp::LeadingZeros {
        let padding = values.literal(Type::I32, u64::from(32 - input_type.bits()));
        fold(
            values,
            Type::I32,
            Expression::Binary {
                operator: Binary::Integer(BinaryOp::Sub),
                left: count,
                right: padding,
            },
        )
    } else {
        count
    };
    // The sentinel bounds CTZ unconditionally. CLZ's adjustment stays nonnegative
    // only for a normalized input: path reduction may have removed its mask
    // under a guard, which cannot narrow a globally shared native expression.
    if operator == BitCountOp::TrailingZeros || values.bounds[input].unsigned <= input_type.bits() {
        values.retain_bounds(result, id);
    }
    fold(
        values,
        value.ty,
        Expression::Unary {
            operator: Unary::Convert,
            input: result,
        },
    )
}

fn fold(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    super::fold::fold_carrier(values, ty, expression, 0)
}

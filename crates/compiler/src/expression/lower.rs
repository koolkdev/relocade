//! Expand compound operations into ordinary, carrier-preserving expressions.
use super::{BinaryOp as Binary, TernaryOp as Ternary, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::{BinaryOp, BitCountOp},
    Expression, Type,
};

mod vector;

pub(crate) fn lower(values: &mut ValueTable, id: usize) -> usize {
    use BitCountOp::{LeadingZeros, TrailingZeros};

    let value = values[id];
    let ValueDefinition::Expression(expression) = value.definition else {
        return id;
    };
    let (operator, input_type, input) = match expression {
        Expression::Unary {
            operator:
                Unary::BitCount {
                    operator: operator @ (LeadingZeros | TrailingZeros),
                    input_type: input_type @ (Type::I1 | Type::I8 | Type::I16),
                },
            input,
        } => (operator, input_type, input),
        Expression::Binary {
            operator: Binary::Swizzle {
                input_type: Type::V64,
            },
            ..
        }
        | Expression::Ternary {
            operator: Ternary::Shuffle { .. },
            ..
        } => return vector::lower(values, value.ty, expression),
        _ => return id,
    };
    // Construction already normalized the input. The sentinel and padding
    // belong to its i32 carrier, not to the narrower logical input type.
    let input = unary(values, Type::I32, Unary::Convert, input);
    let input = if operator == TrailingZeros {
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
    let count = unary(
        values,
        Type::I32,
        Unary::BitCount {
            operator,
            input_type: Type::I32,
        },
        input,
    );
    let result = if operator == LeadingZeros {
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
    if operator == TrailingZeros || values.bounds[input].unsigned <= input_type.bits() {
        values.retain_bounds(result, id);
    }
    unary(values, value.ty, Unary::Convert, result)
}

fn unary(values: &mut ValueTable, ty: Type, operator: Unary, input: usize) -> usize {
    fold(values, ty, Expression::Unary { operator, input })
}

fn fold(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    super::fold::fold_carrier(values, ty, expression, 0)
}

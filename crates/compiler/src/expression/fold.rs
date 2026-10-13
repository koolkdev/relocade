//! Expression normalization and folding over the function's stored values.
//!
//! Construction interprets logical types before folding. Replacing operands
//! folds the existing carrier operations without repeating that normalization.

use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, TernaryOp as Ternary,
    UnaryOp as Unary,
};
use crate::{
    body::{Value, ValueDefinition, ValueTable},
    Expression, Type,
};

mod arithmetic;
mod bits;
mod bitwise;
mod comparisons;
mod construct;
mod lanes;
mod layout;
mod reinterpret;
mod select;
mod shifts;
mod vector;

#[cfg(test)]
mod tests;

pub(crate) use construct::build;

use super::TypedLiteral;

/// Fold a stored carrier operation without repeating logical normalization.
#[cfg(test)]
pub(crate) fn refold(values: &mut ValueTable, id: usize) -> usize {
    let value = values[id];
    let Some(result) = values.expression(id) else {
        return id;
    };
    fold_carrier(values, value.ty, result.expression, result.component)
}

/// Fold a carrier recipe without admitting logical operands again.
pub(crate) fn fold_carrier(
    values: &mut ValueTable,
    ty: Type,
    expression: Expression<usize>,
    component: usize,
) -> usize {
    Folder { values }.fold_result(ty, expression, component)
}

pub(crate) fn normalize(values: &mut ValueTable, input: usize) -> usize {
    Folder { values }.normalize(input)
}

struct Folder<'a> {
    values: &'a mut ValueTable,
}

impl Folder<'_> {
    fn fold(&mut self, ty: Type, expression: Expression<usize>) -> usize {
        self.fold_result(ty, expression, 0)
    }

    // Construction and operand replacement share rewrites that preserve every result bit.
    fn fold_result(&mut self, ty: Type, expression: Expression<usize>, component: usize) -> usize {
        if let Some(bits) = self
            .literals(expression)
            .and_then(|literals| literals.carrier_result(ty, component))
        {
            return self.values.carrier_literal(ty, bits);
        }
        let input = match expression {
            Expression::Unary { operator, input } => match operator {
                Unary::ExtractLane { lane } => self.fold_extract_lane(ty, lane, input),
                Unary::ZeroTest { nonzero } => self.fold_zero_test(input, nonzero),
                Unary::LowBits { bits } => Some(self.fold_low_bits(ty, input, bits)),
                Unary::SignExtend { input_type } => self.fold_sign_extend(ty, input_type, input),
                Unary::Convert if self.values[input].ty == ty => Some(input),
                Unary::Convert => match self.values[input].definition {
                    ValueDefinition::Expression(Expression::Unary {
                        operator: Unary::Convert,
                        input: original,
                    }) if self.values[original].ty == ty
                        && self.values[input].ty.carrier().bits() >= ty.carrier().bits() =>
                    {
                        Some(original)
                    }
                    _ => None,
                },
                Unary::Reinterpret => self.reinterpret_input(input).filter(|&original| {
                    self.values[original].ty == ty
                        && self.values[input].ty.carrier().bits() >= ty.carrier().bits()
                }),
                _ => None,
            },
            Expression::Binary {
                operator,
                left,
                right,
            } => match operator {
                Binary::Integer(operator) => self.fold_binary(ty, operator, left, right),
                Binary::Swizzle { input_type } => self.fold_swizzle(ty, input_type, left, right),
                Binary::Bitwise(operator) => self.fold_bitwise(ty, operator, left, right),
                Binary::Compare(Compare::Integer(operator)) => {
                    self.fold_compare(operator, left, right)
                }
                Binary::Vector {
                    operator,
                    lane_type,
                } => self.fold_vector(ty, operator, lane_type, left, right),
                _ => None,
            },
            Expression::Shift {
                operator,
                value,
                count,
            } => match operator {
                Shift::Scalar(_) => self.fold_shift(ty, value, count),
                Shift::Rotate(_) => self.fold_rotate(ty, value, count),
                Shift::Lanes { lane_type, .. } => self.fold_lane_shift(value, count, lane_type),
            },
            Expression::Ternary {
                operator,
                first,
                second,
                third,
            } => match operator {
                Ternary::Select => self.fold_select(third, first, second),
                Ternary::Bitselect => self.fold_select_bits(ty, third, first, second),
                Ternary::Shuffle { input_type } => {
                    self.fold_shuffle_bytes(ty, input_type, first, second, third)
                }
            },
            Expression::MultiplyWide {
                signed,
                left,
                right,
            } => self.fold_multiply_wide(signed, left, right, component),
        };
        let expression = if let Some(input) = input {
            if self.values[input].ty == ty {
                return input;
            }
            Expression::Unary {
                operator: Unary::Convert,
                input,
            }
        } else {
            expression
        };
        self.intern_result(ty, expression, component)
    }

    fn vector_literal(&self, input: usize) -> Option<u128> {
        match self.values[input].definition {
            ValueDefinition::Literal(bits) => Some(u128::from(bits)),
            _ => None,
        }
    }

    fn literals(&self, expression: Expression<usize>) -> Option<Expression<TypedLiteral>> {
        expression
            .try_map(|&input| {
                let value = self.values[input];
                let ValueDefinition::Literal(bits) = value.definition else {
                    return Err(());
                };
                Ok(TypedLiteral {
                    ty: value.ty,
                    value: bits,
                })
            })
            .ok()
    }

    /// Finish a canonical value without re-entering the rewrite that produced it.
    fn intern(&mut self, ty: Type, expression: Expression<usize>) -> usize {
        self.intern_result(ty, expression, 0)
    }

    fn intern_result(
        &mut self,
        ty: Type,
        expression: Expression<usize>,
        component: usize,
    ) -> usize {
        let id = self.values.intern(Value {
            ty,
            definition: ValueDefinition::Expression(expression),
        });
        let id = self.values.expression_result(id, component);
        if self.values.bounds[id].unsigned == 0 {
            self.values.literal(ty, 0)
        } else {
            id
        }
    }
}

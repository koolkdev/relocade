//! Expression normalization and folding over the function's stored values.
//!
//! Construction interprets logical types before folding. Replacing operands
//! folds the existing carrier operations without repeating that normalization.

use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, UnaryOp as Unary,
};
use crate::{
    body::{Value, ValueDefinition, ValueTable},
    integer::BitCountOp,
    Expression, Type,
};

mod arithmetic;
mod bits;
mod bitwise;
mod comparisons;
mod select;
mod shifts;

#[cfg(test)]
mod tests;

use super::TypedLiteral;

pub(crate) fn build(
    values: &mut ValueTable,
    ty: Type,
    expression: Expression<usize>,
    component: usize,
) -> usize {
    Folder { values }.expression(ty, expression, component)
}

/// Fold a stored carrier operation without repeating logical normalization.
pub(crate) fn refold(values: &mut ValueTable, id: usize) -> usize {
    let value = values[id];
    let Some(result) = values.expression(id) else {
        return id;
    };
    Folder { values }.fold_result(value.ty, result.expression, result.component)
}

pub(crate) fn normalize(values: &mut ValueTable, input: usize) -> usize {
    Folder { values }.normalize(input)
}

struct Folder<'a> {
    values: &'a mut ValueTable,
}

impl Folder<'_> {
    // Inputs carry logical types; canonical operations may retain wider physical bits.
    fn expression(&mut self, ty: Type, expression: Expression<usize>, component: usize) -> usize {
        let expression = expression.map(|&input| {
            let value = self.values[input];
            match value.definition {
                ValueDefinition::Literal(bits) => self.values.literal(value.ty, bits),
                _ => input,
            }
        });
        if let Some(literals) = self.literals(expression) {
            if let Some(bits) = literals.constant_result(ty, component) {
                return self.values.literal(ty, bits);
            }
        }
        match expression {
            Expression::Unary { operator, input } => match operator {
                Unary::SignExtend { input_type } => self.sign_extend(input, input_type, ty),
                Unary::BitCount {
                    operator,
                    input_type,
                } => self.bit_count(operator, input_type, input),
                Unary::ZeroTest { nonzero } => self.zero_test(input, nonzero),
                Unary::Convert => self.convert(input, ty),
                _ => self.fold(ty, expression),
            },
            Expression::Binary {
                operator,
                left,
                right,
            } => match operator {
                Binary::Integer(operator) => self.binary(operator, left, right),
                Binary::Bitwise(operator) => self.bitwise(operator, left, right),
                Binary::Compare(Compare::Integer(operator)) => self.compare(operator, left, right),
                _ => self.fold(ty, expression),
            },
            Expression::Shift {
                operator,
                value,
                count,
            } => match operator {
                Shift::Scalar(operator) => self.shift(operator, value, count),
                Shift::Rotate(operator) => self.rotate(operator, value, count),
                Shift::Lanes { .. } => self.fold(ty, expression),
            },
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => self.select(condition, when_true, when_false),
            Expression::MultiplyWide { .. } => self.fold_result(ty, expression, component),
        }
    }

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
                Unary::ZeroTest { nonzero } => self.fold_zero_test(input, nonzero),
                Unary::LowBits { bits } => Some(self.fold_low_bits(ty, input, bits)),
                Unary::SignExtend { input_type } => self.fold_sign_extend(ty, input_type, input),
                Unary::Convert if self.values[input].ty == ty => Some(input),
                Unary::Convert => match self.values[input].definition {
                    ValueDefinition::Expression(Expression::Unary {
                        operator: Unary::Convert,
                        input: original,
                    }) if self.values[original].ty == ty
                        && self.values[input].ty.bits() >= ty.bits() =>
                    {
                        Some(original)
                    }
                    _ => None,
                },
                Unary::Reinterpret => match self.values[input].definition {
                    ValueDefinition::Expression(Expression::Unary {
                        operator: Unary::Reinterpret,
                        input: original,
                    }) if self.values[original].ty == ty => Some(original),
                    _ => None,
                },
                _ => None,
            },
            Expression::Binary {
                operator,
                left,
                right,
            } => match operator {
                Binary::Integer(operator) => self.fold_binary(ty, operator, left, right),
                Binary::Bitwise(operator) => self.fold_bitwise(ty, operator, left, right),
                Binary::Compare(Compare::Integer(operator)) => {
                    self.fold_compare(operator, left, right)
                }
                _ => None,
            },
            Expression::Shift {
                operator,
                value,
                count,
            } => match operator {
                Shift::Scalar(_) => self.fold_shift(ty, value, count),
                Shift::Rotate(_) => self.fold_rotate(ty, value, count),
                Shift::Lanes { .. } => None,
            },
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => self.fold_select(condition, when_true, when_false),
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

    fn bit_count(&mut self, operator: BitCountOp, input_type: Type, input: usize) -> usize {
        let input = self.fold(
            input_type,
            Expression::Unary {
                operator: Unary::LowBits {
                    bits: input_type.bits(),
                },
                input,
            },
        );
        self.fold(
            input_type,
            Expression::Unary {
                operator: Unary::BitCount {
                    operator,
                    input_type,
                },
                input,
            },
        )
    }
}

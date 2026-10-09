//! Expression normalization and folding over the function's stored values.
//!
//! Construction interprets logical types before folding. Replacing operands
//! folds the existing carrier operations without repeating that normalization.

use crate::{
    body::{Value, ValueDefinition, ValueTable},
    integer::BitCountOp,
    Expression, Type,
};

mod arithmetic;
mod bits;
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
            Expression::Binary {
                operator,
                left,
                right,
            } => self.binary(operator, left, right),
            Expression::Compare {
                operator,
                left,
                right,
            } => self.compare(operator, left, right),
            Expression::Shift {
                operator,
                value,
                count,
            } => self.shift(operator, value, count),
            Expression::Rotate {
                operator,
                value,
                count,
            } => self.rotate(operator, value, count),
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => self.select(condition, when_true, when_false),
            Expression::SignExtend { input } => self.sign_extend(input, ty),
            Expression::BitCount { operator, input } => self.bit_count(operator, input),
            Expression::ZeroTest { input, nonzero } => self.zero_test(input, nonzero),
            Expression::Convert { input } => self.convert(input, ty),
            Expression::LowBits { .. }
            | Expression::FloatBinary { .. }
            | Expression::FloatUnary { .. }
            | Expression::FloatCompare { .. }
            | Expression::Reinterpret { .. } => self.fold(ty, expression),
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
            Expression::Binary {
                operator,
                left,
                right,
            } => self.fold_binary(ty, operator, left, right),
            Expression::MultiplyWide {
                signed,
                left,
                right,
            } => self.fold_multiply_wide(signed, left, right, component),
            Expression::Shift { value, count, .. } => self.fold_shift(ty, value, count),
            Expression::Rotate { value, count, .. } => self.fold_rotate(ty, value, count),
            Expression::Compare {
                operator,
                left,
                right,
            } => self.fold_compare(operator, left, right),
            Expression::ZeroTest { input, nonzero } => self.fold_zero_test(input, nonzero),
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => self.fold_select(condition, when_true, when_false),
            Expression::LowBits { input, bits } => Some(self.fold_low_bits(ty, input, bits)),
            Expression::SignExtend { input } => self.fold_sign_extend(ty, input),
            Expression::Convert { input } if self.values[input].ty == ty => Some(input),
            Expression::Convert { input: source } => match self.values[source].definition {
                ValueDefinition::Expression(Expression::Convert { input })
                    if self.values[input].ty == ty
                        && self.values[source].ty.bits() >= ty.bits() =>
                {
                    Some(input)
                }
                _ => None,
            },
            Expression::Reinterpret { input } => match self.values[input].definition {
                ValueDefinition::Expression(Expression::Reinterpret { input: original })
                    if self.values[original].ty == ty =>
                {
                    Some(original)
                }
                _ => None,
            },
            // Integer identities and comparison complements do not hold for
            // floating-point values, including NaNs and signed zeros.
            Expression::FloatBinary { .. }
            | Expression::FloatUnary { .. }
            | Expression::FloatCompare { .. }
            | Expression::BitCount { .. } => None,
        };
        let expression = if let Some(input) = input {
            if self.values[input].ty == ty {
                return input;
            }
            Expression::Convert { input }
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

    /// Finish a canonical scalar without re-entering the rewrite that produced it.
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

    fn bit_count(&mut self, operator: BitCountOp, input: usize) -> usize {
        let value = self.values[input];
        let input = self.normalize(input);
        self.fold(value.ty, Expression::BitCount { operator, input })
    }
}

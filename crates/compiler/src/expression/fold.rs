//! Expression normalization and folding over the function's stored values.
//!
//! Construction interprets logical types before folding. Replacing operands
//! folds the existing carrier operations without repeating that normalization.

use crate::{
    body::{Value, ValueDefinition, ValueTable},
    integer::{self, BitCountOp},
    Expression, Type,
};

mod arithmetic;
mod bits;
mod comparisons;
mod shifts;

pub(crate) fn build(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    Folder { values }.expression(ty, expression)
}

/// Replace a calculation's inputs and fold without repeating logical normalization.
pub(crate) fn map_inputs(
    values: &mut ValueTable,
    id: usize,
    input: impl FnMut(&usize) -> usize,
) -> usize {
    let value = values[id];
    let ValueDefinition::Expression(expression) = value.definition else {
        return id;
    };
    Folder { values }.fold(value.ty, expression.map(input))
}

pub(crate) fn normalize(values: &mut ValueTable, input: usize) -> usize {
    Folder { values }.normalize(input)
}

struct Folder<'a> {
    values: &'a mut ValueTable,
}

impl Folder<'_> {
    // Inputs carry logical types; canonical operations may retain wider physical bits.
    fn expression(&mut self, ty: Type, expression: Expression<usize>) -> usize {
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
            Expression::LowBits { input, bits } => self.low_bits(input, bits),
        }
    }

    // Construction and operand replacement share rewrites that preserve every result bit.
    fn fold(&mut self, ty: Type, expression: Expression<usize>) -> usize {
        let input = match expression {
            Expression::Binary {
                operator,
                left,
                right,
            } => self.fold_binary(ty, operator, left, right),
            Expression::Convert { input: source } => match self.values[source].definition {
                ValueDefinition::Expression(Expression::Convert { input })
                    if self.values[input].ty == ty
                        && self.values[source].ty.bits() >= ty.bits() =>
                {
                    Some(input)
                }
                _ => None,
            },
            _ => None,
        };
        let expression = if let Some(input) = input {
            if self.values[input].ty == ty {
                return input;
            }
            Expression::Convert { input }
        } else {
            expression
        };
        let id = self.values.intern(Value {
            ty,
            definition: ValueDefinition::Expression(expression),
        });
        if self.values.bounds[id].unsigned == 0 {
            self.values.constant(ty, 0)
        } else {
            id
        }
    }

    fn select(&mut self, condition: usize, when_true: usize, when_false: usize) -> usize {
        let condition = self.normalize(condition);
        match self.values[condition].definition {
            ValueDefinition::Constant(0) => return when_false,
            ValueDefinition::Constant(_) => return when_true,
            _ if when_true == when_false => return when_true,
            _ => {}
        }
        self.fold(
            self.values[when_true].ty,
            Expression::Select {
                condition,
                when_true,
                when_false,
            },
        )
    }

    fn bit_count(&mut self, operator: BitCountOp, input: usize) -> usize {
        let value = self.values[input];
        if let ValueDefinition::Constant(bits) = value.definition {
            return self
                .values
                .constant(value.ty, integer::bit_count(value.ty, operator, bits));
        }
        let input = self.normalize(input);
        self.fold(value.ty, Expression::BitCount { operator, input })
    }
}

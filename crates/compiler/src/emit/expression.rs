//! Dispatch pure expression shapes to opcode lowering.
use super::Writer;
use crate::{Expression, Type};

mod float_vector;
mod operators;
mod vector;
mod wide;

impl Writer<'_> {
    /// Inputs are on the stack in Wasm order. Keep their logical types: predicates
    /// use operand carriers and layout operations retain scalar lane widths.
    /// Conversions within one carrier have already been bypassed by selection.
    pub(super) fn expression(&mut self, result_type: Type, expression: Expression<Type>) {
        match expression {
            Expression::Unary { operator, input } => self.unary(operator, input, result_type),
            Expression::Binary { operator, left, .. } => {
                self.emit(operators::binary(operator, left))
            }
            Expression::Shift { operator, .. } => {
                self.emit(operators::shift(operator, result_type))
            }
            Expression::Ternary { operator, .. } => self.emit(operators::ternary(operator)),
            Expression::MultiplyWide { signed, .. } => self.multiply_wide(signed),
        }
    }
}

//! Checked integer division, including guest fault exits before result writes.

use wasm86_compiler::{BuildError, Val};

use crate::{alu::DoubleWidth, state::exit};

use super::ExecutionBuilder;

#[derive(Clone, Copy)]
pub(crate) enum DivideOp {
    Unsigned,
    Signed,
}

pub(crate) struct Division<T: DoubleWidth> {
    pub(crate) quotient: Val<T>,
    pub(crate) remainder: Val<T>,
}

impl ExecutionBuilder<'_, '_> {
    /// The dividend contains both original halves. All operand access must
    /// succeed before calling this method; neither result is written here.
    pub(crate) fn divide<T: DoubleWidth>(
        &mut self,
        operation: DivideOp,
        dividend: Val<T::Double>,
        divisor: Val<T>,
    ) -> Result<Division<T>, BuildError> {
        let width = T::BYTES * 8;
        let invalid = match operation {
            // H*2^width + L fits a width-bit quotient exactly when H < divisor.
            // This also rejects a zero divisor before either Wasm operation.
            DivideOp::Unsigned => dividend
                .unsigned()
                .shr(width)
                .truncate::<T>()
                .unsigned()
                .ge(&divisor),
            DivideOp::Signed => {
                let minimum = Val::<T::Double>::from(1).shl(width * 2 - 1);
                divisor.eq(0).or(dividend.eq(minimum).and(divisor.eq(-1)))
            }
        };
        self.fault_if(invalid, exit::divide_error())?;
        let divisor = match operation {
            DivideOp::Unsigned => divisor.unsigned().extend::<T::Double>(),
            DivideOp::Signed => divisor.signed().extend::<T::Double>(),
        };
        let quotient = match operation {
            DivideOp::Unsigned => dividend.unsigned().div(&divisor),
            DivideOp::Signed => dividend.signed().div(&divisor),
        };
        if let DivideOp::Signed = operation {
            // The wider computation is safe, but its quotient can still exceed
            // the signed destination range. Check before truncating or writing.
            self.fault_if(
                quotient.ne(quotient.truncate::<T>().signed().extend::<T::Double>()),
                exit::divide_error(),
            )?;
        }
        let remainder = match operation {
            DivideOp::Unsigned => dividend.unsigned().rem(&divisor),
            DivideOp::Signed => dividend.signed().rem(&divisor),
        };
        Ok(Division {
            quotient: quotient.truncate::<T>(),
            remainder: remainder.truncate::<T>(),
        })
    }
}

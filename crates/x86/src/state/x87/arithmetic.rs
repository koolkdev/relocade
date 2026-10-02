//! Register arithmetic resolves exceptions before committing its value and pop.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I32, I8};

use crate::x87::{ArithmeticResult, ExtendedValue, RoundingMode};

use super::{control::Exception, X87State};

impl X87State {
    pub(crate) fn binary_register(
        &mut self,
        body: &mut BlockBuilder<'_>,
        destination: Val<I32>,
        source: Val<I32>,
        pop: bool,
        calculate: impl FnOnce(
            &ExtendedValue,
            &ExtendedValue,
            Val<I8>,
            &RoundingMode,
        ) -> ArithmeticResult,
    ) -> Result<(), BuildError> {
        // Both operands use the old TOP, including when they alias each other.
        let left = self.read_stack(body, &destination)?;
        let right = self.read_stack(body, source)?;
        let stack_fault = left.empty.or(&right.empty);
        let precision = self.control.precision(body)?;
        let rounding = self.control.rounding(body)?;
        let result = calculate(
            &left.value.or_indefinite(&stack_fault),
            &right.value.or_indefinite(&stack_fault),
            precision,
            &rounding,
        );
        let invalid = stack_fault.or(&result.invalid);
        let unmasked_invalid =
            self.status
                .record_exception(body, Exception::Invalid, &invalid, &mut self.control)?;
        let unmasked_denormal = self.status.record_exception(
            body,
            Exception::Denormal,
            &result.denormal,
            &mut self.control,
        )?;
        self.status.record_stack_fault(body, &stack_fault)?;
        let suppressed = unmasked_invalid.or(unmasked_denormal);
        let enabled = suppressed.eq(false);
        let overflow = enabled.and(&result.overflow);
        let unmasked_overflow = self.status.record_exception(
            body,
            Exception::Overflow,
            &overflow,
            &mut self.control,
        )?;
        let tiny = enabled.and(&result.tiny);
        let underflow_unmasked = self.control.unmasked(body, Exception::Underflow)?;
        let rounded = result.resolve_range(&unmasked_overflow.or(tiny.and(&underflow_unmasked)));
        let underflow = tiny.and(underflow_unmasked.or(&rounded.inexact));
        let unmasked_underflow = self.status.record_exception(
            body,
            Exception::Underflow,
            &underflow,
            &mut self.control,
        )?;
        let precision = enabled.and(rounded.inexact);
        let unmasked_precision = self.status.record_exception(
            body,
            Exception::Precision,
            &precision,
            &mut self.control,
        )?;
        self.status.set_c1(body, enabled.and(rounded.incremented))?;
        self.status.record_pending_exception(
            body,
            suppressed
                .or(unmasked_overflow)
                .or(unmasked_underflow)
                .or(unmasked_precision),
        )?;
        // Unlike memory stores, unmasked range and precision exceptions commit
        // register results, including the pop and any adjusted exponent.
        self.write_stack(body, destination, &rounded.value, &enabled)?;
        if pop {
            self.pop(body, &enabled)?;
        }
        Ok(())
    }
}

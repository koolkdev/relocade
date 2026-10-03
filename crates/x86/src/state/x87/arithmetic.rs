//! Register arithmetic resolves exceptions before committing its value and pop.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I8};

use crate::x87::{ArithmeticResult, ExtendedValue, RoundingMode};

use super::{control::Exception, X87State};

pub(crate) struct RegisterArithmetic {
    destination: Val<I32>,
    pop: bool,
    stack_fault: Val<I1>,
    left: ExtendedValue,
    right: ExtendedValue,
    precision: Val<I8>,
    rounding: RoundingMode,
}

impl X87State {
    /// Captures the old operands and controls before any instruction effects.
    pub(crate) fn prepare_binary_register(
        &mut self,
        body: &mut BlockBuilder<'_>,
        destination: Val<I32>,
        source: Val<I32>,
        pop: bool,
    ) -> Result<RegisterArithmetic, BuildError> {
        // Both operands use the old TOP, including when they alias each other.
        let left = self.read_stack(body, &destination)?;
        let right = self.read_stack(body, source)?;
        let stack_fault = left.empty.or(&right.empty);
        let precision = self.control.precision(body)?;
        let rounding = self.control.rounding(body)?;
        let left = left.value.or_indefinite(&stack_fault);
        let right = right.value.or_indefinite(&stack_fault);
        Ok(RegisterArithmetic {
            destination,
            pop,
            stack_fault,
            left,
            right,
            precision,
            rounding,
        })
    }
}

impl RegisterArithmetic {
    /// Numerical operations consume values and controls without changing state.
    pub(crate) fn calculate<R>(
        &self,
        calculate: impl FnOnce(&ExtendedValue, &ExtendedValue, Val<I8>, &RoundingMode) -> R,
    ) -> R {
        calculate(
            &self.left,
            &self.right,
            self.precision.clone(),
            &self.rounding,
        )
    }

    pub(crate) fn operands_present(&self) -> Val<I1> {
        self.stack_fault.eq(false)
    }

    /// Requires a guard establishing `operands_present()` on the continuing path.
    pub(crate) fn assume_present(&mut self) {
        self.stack_fault = false.into();
    }

    /// Commits the arithmetic response after the instruction's restart guards.
    pub(crate) fn commit(
        self,
        body: &mut BlockBuilder<'_>,
        state: &mut X87State,
        result: ArithmeticResult,
    ) -> Result<(), BuildError> {
        let Self {
            destination,
            pop,
            stack_fault,
            ..
        } = self;
        let invalid = stack_fault.or(&result.invalid);
        let unmasked_invalid = state.status.record_exception(
            body,
            Exception::Invalid,
            &invalid,
            &mut state.control,
        )?;
        let unmasked_denormal = state.status.record_exception(
            body,
            Exception::Denormal,
            &result.denormal,
            &mut state.control,
        )?;
        state.status.record_stack_fault(body, &stack_fault)?;
        let suppressed = unmasked_invalid.or(unmasked_denormal);
        let enabled = suppressed.eq(false);
        let overflow = enabled.and(&result.overflow);
        let unmasked_overflow = state.status.record_exception(
            body,
            Exception::Overflow,
            &overflow,
            &mut state.control,
        )?;
        let tiny = enabled.and(&result.tiny);
        let underflow_unmasked = state.control.unmasked(body, Exception::Underflow)?;
        let rounded = result.resolve_range(&unmasked_overflow.or(tiny.and(&underflow_unmasked)));
        let underflow = tiny.and(underflow_unmasked.or(&rounded.inexact));
        let unmasked_underflow = state.status.record_exception(
            body,
            Exception::Underflow,
            &underflow,
            &mut state.control,
        )?;
        let precision = enabled.and(rounded.inexact);
        let unmasked_precision = state.status.record_exception(
            body,
            Exception::Precision,
            &precision,
            &mut state.control,
        )?;
        state
            .status
            .set_c1(body, enabled.and(rounded.incremented))?;
        state.status.record_pending_exception(
            body,
            suppressed
                .or(unmasked_overflow)
                .or(unmasked_underflow)
                .or(unmasked_precision),
        )?;
        // Unlike memory stores, unmasked range and precision exceptions commit
        // register results, including the pop and any adjusted exponent.
        state.write_stack(body, destination, &rounded.value, &enabled)?;
        if pop {
            state.pop(body, &enabled)?;
        }
        Ok(())
    }
}

//! Register arithmetic resolves exceptions before committing its value and pop.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I8};

use crate::x87::{ArithmeticResult, ExtendedValue, RoundingMode};

use super::{control::Exception, X87State};

pub(crate) struct RegisterArithmetic {
    destination: Val<I32>,
    pop: bool,
    stack_fault: Val<I1>,
    normal_operands: Val<I1>,
    result: ArithmeticResult,
}

impl X87State {
    /// Reads the old stack and constructs pure arithmetic before any effects.
    pub(crate) fn prepare_binary_register(
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
    ) -> Result<RegisterArithmetic, BuildError> {
        // Both operands use the old TOP, including when they alias each other.
        let left = self.read_stack(body, &destination)?;
        let right = self.read_stack(body, source)?;
        let stack_fault = left.empty.or(&right.empty);
        let normal_operands = stack_fault
            .eq(false)
            .and(left.value.normal())
            .and(right.value.normal());
        let precision = self.control.precision(body)?;
        let rounding = self.control.rounding(body)?;
        let result = calculate(
            &left.value.or_indefinite(&stack_fault),
            &right.value.or_indefinite(&stack_fault),
            precision,
            &rounding,
        );
        Ok(RegisterArithmetic {
            destination,
            pop,
            stack_fault,
            normal_operands,
            result,
        })
    }
}

impl RegisterArithmetic {
    pub(crate) fn normal_operands(&self) -> Val<I1> {
        self.normal_operands.clone()
    }

    pub(crate) fn in_range(&self) -> Val<I1> {
        self.result.in_range.clone()
    }

    /// Requires guards proving a nonempty stack and a normal result with only
    /// a possible precision exception. Normal inputs alone are insufficient:
    /// the operation must also establish its result's class and range.
    pub(crate) fn assume_normal_result(&mut self) {
        self.stack_fault = false.into();
        self.result.assume_normal_result();
    }

    /// Commits the arithmetic response after the instruction's restart guards.
    pub(crate) fn commit(
        self,
        body: &mut BlockBuilder<'_>,
        state: &mut X87State,
    ) -> Result<(), BuildError> {
        let Self {
            destination,
            pop,
            stack_fault,
            result,
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

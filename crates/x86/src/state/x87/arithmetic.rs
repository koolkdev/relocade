//! Arithmetic resolves exceptions before committing its register result and pop.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I8};

use crate::x87::{
    ArithmeticCandidate, ArithmeticResult, BinaryOperand, BinaryOperands, BinaryOperation,
    RoundingMode,
};

use super::{control::Exception, X87Access};

pub(crate) enum ArithmeticSource {
    Register(Val<I32>),
    Binary(BinaryOperand),
}

pub(crate) struct Arithmetic {
    destination: Val<I32>,
    pop: bool,
    stack_fault: Val<I1>,
    operands: BinaryOperands,
    precision: Val<I8>,
    rounding: RoundingMode,
}

impl X87Access<'_, '_> {
    /// Captures the old operands and controls before any instruction effects.
    pub(crate) fn prepare_binary(
        &mut self,
        destination: Val<I32>,
        source: ArithmeticSource,
        pop: bool,
    ) -> Result<Arithmetic, BuildError> {
        // Both operands use the old TOP, including when they alias each other.
        let left = self.read_stack(&destination)?;
        let (operands, stack_fault) = match source {
            ArithmeticSource::Register(index) => {
                let right = self.read_stack(index)?;
                (
                    BinaryOperands::new(&left.value, &right.value),
                    left.empty.or(right.empty),
                )
            }
            ArithmeticSource::Binary(source) => (
                BinaryOperands::from_binary(&left.value, &source),
                left.empty,
            ),
        };
        let precision = self.state.control.precision(self.body)?;
        let rounding = self.state.control.rounding(self.body)?;
        Ok(Arithmetic {
            destination,
            pop,
            stack_fault,
            operands,
            precision,
            rounding,
        })
    }
}

impl Arithmetic {
    /// Numerical selection may use PC53/nearest only after execution guards it.
    pub(crate) fn rounding_candidate(
        &self,
        body: &mut BlockBuilder<'_>,
        operation: BinaryOperation,
        nearest_53: bool,
    ) -> Result<ArithmeticCandidate, BuildError> {
        self.operands.rounding_candidate(
            body,
            operation,
            self.precision.clone(),
            &self.rounding,
            nearest_53,
        )
    }

    /// Numerical operations consume values and controls without changing state.
    pub(crate) fn calculate(
        &self,
        body: &mut BlockBuilder<'_>,
        operation: BinaryOperation,
    ) -> Result<ArithmeticResult, BuildError> {
        Ok(self
            .operands
            .calculate(body, operation, self.precision.clone(), &self.rounding)?
            .result)
    }

    /// Present operands admitted for this operation need no pre-operation response.
    /// The result still needs a separate range check before precision-only use.
    pub(crate) fn precision_only_operands(&self, operation: BinaryOperation) -> Val<I1> {
        self.stack_fault
            .eq(false)
            .and(self.operands.precision_only(operation))
    }

    /// Requires the operation's `precision_only_operands` guard on this path.
    pub(crate) fn assume_present(&mut self) {
        self.stack_fault = false.into();
    }
}

impl X87Access<'_, '_> {
    /// Commits the arithmetic response after the instruction's restart guards.
    pub(crate) fn commit_arithmetic(
        &mut self,
        arithmetic: Arithmetic,
        result: ArithmeticResult,
    ) -> Result<(), BuildError> {
        let Arithmetic {
            destination,
            pop,
            stack_fault,
            ..
        } = arithmetic;
        let Self { state, body } = self;
        // Stack faults override all numerical responses, including #D from a
        // narrow source that expanded to a normal binary80 value.
        let result = result.or_indefinite(&stack_fault);
        let unmasked_invalid = state.status.record_exception(
            body,
            Exception::Invalid,
            &result.invalid,
            &mut state.control,
        )?;
        let unmasked_zero_divide = state.status.record_exception(
            body,
            Exception::ZeroDivide,
            &result.zero_divide,
            &mut state.control,
        )?;
        let unmasked_denormal = state.status.record_exception(
            body,
            Exception::Denormal,
            &result.denormal,
            &mut state.control,
        )?;
        state.status.record_stack_fault(body, &stack_fault)?;
        let suppressed = unmasked_invalid
            .or(unmasked_zero_divide)
            .or(unmasked_denormal);
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
        self.write_stack(destination, &rounded.value, &enabled)?;
        if pop {
            self.pop(&enabled)?;
        }
        Ok(())
    }
}

//! Arithmetic reads, guards, calculates and updates state in one operation.

use wasm86_compiler::{BuildError, Val, I32};

use crate::{
    state::x87::{Exception, StackValue, X87Specialization},
    x87::{ArithmeticResult, BinaryOperation},
};

use super::{operand::X87Operands, ExecutionBuilder, X87Operand};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn arithmetic_x87(
        &mut self,
        destination: Val<I32>,
        source: X87Operand,
        operation: BinaryOperation,
        pop: bool,
    ) -> Result<(), BuildError> {
        self.check_x87_exception()?;
        let X87Operands {
            values: operands,
            mut stack_fault,
            memory,
        } = self.read_x87_operands(destination.clone(), source)?;
        let precision = self.state.x87.control.precision(&mut self.body)?;
        let rounding = self.state.x87.control.rounding(&mut self.body)?;
        let result = if self.can_specialize {
            self.specialize_x87(X87Specialization::Arithmetic)?;
            self.specialize_on(
                stack_fault
                    .eq(false)
                    .and(operands.precision_only(operation)),
            )?;
            stack_fault = false.into();

            // The control guard establishes this fact about the current SSA state.
            // Numerical selection does not need the CPU snapshot or restart policy.
            let nearest_53 = self.observed_cpu.is_some_and(|observed| {
                let control = &observed.x87.control;
                control.precision_control & 3 == 2 && control.rounding_control & 3 == 0
            });
            let candidate = operands.rounding_candidate(
                &mut self.body,
                operation,
                precision,
                &rounding,
                nearest_53,
            )?;
            self.specialize_on(candidate.valid)?;
            ArithmeticResult::from_rounding(candidate.rounded)
        } else {
            operands
                .calculate(&mut self.body, operation, precision, &rounding)?
                .result
        };
        self.record_x87_operand(memory.as_ref())?;

        let state = &mut self.state.x87;
        let body = &mut self.body;
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
        // Unlike memory stores, unmasked range and precision exceptions write
        // register results, including the pop and any adjusted exponent.
        let mut x87 = state.access(body);
        x87.write_stack(
            destination,
            &StackValue::from_value(rounded.value),
            &enabled,
        )?;
        if pop {
            x87.pop(1, &enabled)?;
        }
        Ok(())
    }
}

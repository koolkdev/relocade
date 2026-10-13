//! Arithmetic register destinations share exception, rounding, and stack effects.

use wasm86_compiler::{BuildError, Val, I1, I32};

use crate::{
    execution::ExecutionBuilder,
    state::x87::{ConditionCode, Exception, StackValue},
    x87::ArithmeticResult,
};

/// Applies the register arithmetic response, including C1 and an optional pop.
/// Unmasked operand exceptions suppress effects; range and precision exceptions
/// still write their register result and defer delivery to the next x87 check.
pub(super) fn write_arithmetic_result(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: Val<I32>,
    result: ArithmeticResult,
    stack_fault: &Val<I1>,
    pop: bool,
) -> Result<(), BuildError> {
    let mut x87 = execution.x87();
    // Stack faults override all numerical responses, including #D from a
    // narrow source that expanded to a normal binary80 value.
    let result = result.or_indefinite(stack_fault);
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &result.invalid)?;
    let unmasked_zero_divide = x87.record_exception(Exception::ZeroDivide, &result.zero_divide)?;
    let unmasked_denormal = x87.record_exception(Exception::Denormal, &result.denormal)?;
    x87.record_stack_fault(stack_fault)?;
    let suppressed = unmasked_invalid
        .or(unmasked_zero_divide)
        .or(unmasked_denormal);
    let enabled = suppressed.eq(false);
    let overflow = enabled.and(&result.overflow);
    let unmasked_overflow = x87.record_exception(Exception::Overflow, &overflow)?;
    let tiny = enabled.and(&result.tiny);
    let underflow_unmasked = x87.exception_unmasked(Exception::Underflow)?;
    let rounded = result.resolve_range(&unmasked_overflow.or(tiny.and(&underflow_unmasked)));
    let underflow = tiny.and(underflow_unmasked.or(&rounded.inexact));
    let unmasked_underflow = x87.record_exception(Exception::Underflow, &underflow)?;
    let precision = enabled.and(rounded.inexact);
    let unmasked_precision = x87.record_exception(Exception::Precision, &precision)?;
    x87.set_condition_code(ConditionCode::C1, enabled.and(rounded.incremented))?;
    x87.record_pending_exception(
        suppressed
            .or(unmasked_overflow)
            .or(unmasked_underflow)
            .or(unmasked_precision),
    )?;
    // Unlike memory stores, unmasked range and precision exceptions write
    // register results, including the pop and any adjusted exponent.
    x87.write_stack(
        destination,
        &StackValue::from_value(rounded.value),
        &enabled,
    )?;
    if pop {
        x87.pop(&enabled)?;
    }
    Ok(())
}

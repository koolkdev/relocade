//! Arithmetic register destinations share exception, rounding, and stack effects.

use wasm86_compiler::{BuildError, Val, I1, I32};

use crate::{
    execution::ExecutionBuilder,
    state::x87::{ConditionCode, Exception, StackValue},
    state::X87Access,
    x87::{ArithmeticResult, ExtendedValue},
};

pub(super) enum Destination {
    Register { index: Val<I32>, pop: bool },
    Conditional { index: Val<I32>, when: Val<I1> },
}

impl Destination {
    fn write(
        self,
        x87: &mut X87Access<'_, '_>,
        value: ExtendedValue,
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        let value = StackValue::from_value(value);
        match self {
            Self::Register { index, pop } => {
                x87.write_stack(index, &value, enabled)?;
                if pop {
                    x87.pop(enabled)?;
                }
                Ok(())
            }
            Self::Conditional { index, when } => {
                x87.write_stack_when(index, &value, &when, enabled)
            }
        }
    }
}

/// Applies the register arithmetic response, including C1 and an optional pop.
/// Unmasked operand exceptions suppress effects; range and precision exceptions
/// still write their register result and defer delivery to the next x87 check.
pub(super) fn write_arithmetic_result(
    execution: &mut ExecutionBuilder<'_, '_>,
    result: ArithmeticResult,
    stack_fault: &Val<I1>,
    destination: Destination,
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
    destination.write(&mut x87, rounded.value, &enabled)
}

//! FXTRACT replaces ST(0) with the exponent and pushes the significand.

use super::*;
use crate::{
    state::x87::{ConditionCode, Exception, StackValue},
    x87::Operand,
};

instruction_families! {
    FXTRACT {
        execute: extract();
        forms { 0xD9 @ 0xF4 => no_operands(); }
    }
}

fn extract(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let top = execution.x87().read_stack(0)?;
    let empty = top.is_empty();
    let mut value = top.value;
    let full = execution.x87().push_full()?;
    let mut fault = empty.or(&full);
    let mut overflow = empty.eq(false).and(full);
    if execution.specialize_x87_operands(&mut fault, value.classification().normal())? {
        value = value.assume_normal();
        overflow = false.into();
    }
    let result = execution.calculate(|body| Operand::new(&value).extract(body))?;
    execution.record_x87_instruction()?;
    let mut x87 = execution.x87();
    let invalid = fault.or(result.invalid);
    let zero_divide = fault.eq(false).and(result.zero_divide);
    let denormal = fault.eq(false).and(result.denormal);
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &invalid)?;
    let unmasked_zero_divide = x87.record_exception(Exception::ZeroDivide, &zero_divide)?;
    let unmasked_denormal = x87.record_exception(Exception::Denormal, &denormal)?;
    x87.record_stack_fault(&fault)?;
    x87.set_condition_code(ConditionCode::C1, overflow)?;
    let suppressed = unmasked_invalid
        .or(unmasked_zero_divide)
        .or(unmasked_denormal);
    x87.record_pending_exception(suppressed.clone())?;
    let enabled = suppressed.eq(false);
    let exponent = result.exponent.or_indefinite(&fault);
    let significand = result.significand.or_indefinite(&fault);
    // Both destination slots use the entry TOP. The push advances it only
    // after the original top has received its exponent response.
    x87.write_stack(0, &StackValue::from_value(exponent), &enabled)?;
    x87.push(&significand, &enabled)
}

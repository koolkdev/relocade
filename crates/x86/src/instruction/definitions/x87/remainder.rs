//! Remainders report partial reduction and quotient bits without changing TOP or ST(1).

use super::*;
use crate::{
    state::x87::{ConditionCode, Exception, StackValue},
    x87::{BinaryOperands, QuotientRounding},
};

instruction_families! {
    FPREM {
        execute: remainder(QuotientRounding::Truncate);
        forms { 0xD9 @ 0xF8 => no_operands(); }
    }
    FPREM1 {
        execute: remainder(QuotientRounding::NearestEven);
        forms { 0xD9 @ 0xF5 => no_operands(); }
    }
}

fn remainder(
    execution: &mut ExecutionBuilder<'_, '_>,
    rounding: QuotientRounding,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let left = execution.x87().read_stack(0)?;
    let right = execution.x87().read_stack(1)?;
    let mut empty = left.is_empty().or(right.is_empty());
    let operands = BinaryOperands::new(&left.value, &right.value);
    execution.specialize_x87_operands(
        &mut empty,
        operands
            .normal_or_zero()
            .and(right.value.classification().zero().eq(false)),
    )?;
    let mut remainder = execution.calculate(|body| operands.remainder(body, rounding))?;
    execution.specialize(|execution| {
        remainder.arithmetic =
            execution.admit_x87_candidate(remainder.arithmetic.in_range.clone())?;
        Ok(())
    })?;
    execution.record_x87_instruction()?;

    let mut x87 = execution.x87();
    let result = remainder.arithmetic.or_indefinite(&empty);
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &result.invalid)?;
    let unmasked_denormal = x87.record_exception(Exception::Denormal, &result.denormal)?;
    let suppressed = unmasked_invalid.or(unmasked_denormal);
    let enabled = suppressed.eq(false);
    x87.record_stack_fault(&empty)?;
    // An exact tiny remainder raises #U only when unmasked. It still writes
    // its adjusted result and the quotient bits, without raising #P.
    let underflow = enabled
        .and(result.tiny.clone())
        .and(x87.exception_unmasked(Exception::Underflow)?);
    let unmasked_underflow = x87.record_exception(Exception::Underflow, &underflow)?;
    let rounded = result.resolve_range(&unmasked_underflow);
    x87.record_pending_exception(suppressed.or(unmasked_underflow))?;
    x87.write_stack(0, &StackValue::from_value(rounded.value), &enabled)?;

    let incomplete = empty.eq(false).and(remainder.incomplete);
    x87.set_condition_code_when(ConditionCode::C2, incomplete.clone(), &enabled)?;
    let complete = enabled.and(incomplete.eq(false));
    for (code, bit) in [
        (ConditionCode::C0, 2),
        (ConditionCode::C3, 1),
        (ConditionCode::C1, 0),
    ] {
        x87.set_condition_code_when(
            code,
            empty
                .eq(false)
                .and(remainder.quotient.and(1_u64 << bit).ne(0_u64)),
            &complete,
        )?;
    }
    x87.set_condition_code_when(ConditionCode::C1, false, &empty)
}

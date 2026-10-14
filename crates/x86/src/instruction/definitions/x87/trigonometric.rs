//! Trigonometric definitions own range flags and register destination policy.

use super::*;
use crate::{
    state::x87::{ConditionCode, X87Specialization},
    x87::{Operand, TrigonometricArgument, TrigonometricFunction},
};

instruction_families! {
    FSIN {
        execute: trigonometric(TrigonometricFunction::Sine);
        forms { 0xD9 @ 0xFE => no_operands(); }
    }
    FCOS {
        execute: trigonometric(TrigonometricFunction::Cosine);
        forms { 0xD9 @ 0xFF => no_operands(); }
    }
}

fn trigonometric(
    execution: &mut ExecutionBuilder<'_, '_>,
    function: TrigonometricFunction,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let top = execution.x87().read_stack(0)?;
    let mut empty = top.is_empty();
    let operand = Operand::new(&top.value);
    let rounding = execution.x87().rounding()?;
    execution.specialize_x87(X87Specialization::Rounding)?;
    execution.specialize_x87_operands(
        &mut empty,
        operand.normal_or_zero().and(operand.trigonometric_domain()),
    )?;
    let mut result = execution.calculate(|body| {
        TrigonometricArgument::new(body, &operand)?.evaluate(body, function, &rounding)
    })?;
    execution.specialize(|execution| {
        result = execution.admit_x87_candidate(result.in_range.clone())?;
        Ok(())
    })?;
    execution.record_x87_instruction()?;
    execution.x87().set_condition_code(
        ConditionCode::C2,
        empty.eq(false).and(operand.trigonometric_out_of_range()),
    )?;
    result::write_arithmetic_result(execution, 0.into(), result, &empty, false)
}

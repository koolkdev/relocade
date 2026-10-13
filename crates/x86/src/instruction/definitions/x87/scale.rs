//! FSCALE multiplies ST(0) by 2^trunc(ST(1)), retaining both stack positions.

use super::*;
use crate::x87::{ArithmeticResult, BinaryOperands};

instruction_families! {
    FSCALE {
        execute: scale();
        forms { 0xD9 @ 0xFD => no_operands(); }
    }
}

fn scale(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let left = execution.x87().read_stack(0)?;
    let right = execution.x87().read_stack(1)?;
    let mut empty = left.is_empty().or(right.is_empty());
    let operands = BinaryOperands::new(&left.value, &right.value);
    let rounding = execution.x87().rounding()?;
    let specialized = execution.specialize(|execution| {
        // An in-range scale is exact and independent of both PC and RC.
        execution.specialize_on(empty.eq(false).and(operands.normal_or_zero()))?;
        empty = false.into();
        let candidate = operands.scale(&rounding).in_range;
        execution.specialize_on(candidate.valid)?;
        Ok(ArithmeticResult::from_rounding(candidate.rounded))
    })?;
    let result = match specialized {
        Some(result) => result,
        None => operands.scale(&rounding),
    };
    execution.record_x87_instruction()?;
    result::write_arithmetic_result(execution, 0.into(), result, &empty, false)
}

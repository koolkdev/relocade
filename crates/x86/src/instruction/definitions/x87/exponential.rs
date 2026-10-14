//! F2XM1 uses full extended precision under RC, independently of PC.

use super::*;
use crate::{state::x87::X87Specialization, x87::Operand};

instruction_families! {
    F2XM1 {
        execute: exponential_minus_one();
        forms { 0xD9 @ 0xF0 => no_operands(); }
    }
}

fn exponential_minus_one(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let top = execution.x87().read_stack(0)?;
    let mut empty = top.is_empty();
    let operand = Operand::new(&top.value);
    let rounding = execution.x87().rounding()?;
    execution.specialize_x87(X87Specialization::Rounding)?;
    execution.specialize_x87_operands(
        &mut empty,
        operand.normal_or_zero().and(operand.exponential_domain()),
    )?;
    let mut result = execution.calculate(|body| operand.exponential_minus_one(body, &rounding))?;
    execution.specialize(|execution| {
        result = execution.admit_x87_candidate(result.in_range.clone())?;
        Ok(())
    })?;
    execution.record_x87_instruction()?;
    result::write_arithmetic_result(execution, 0.into(), result, &empty, false)
}

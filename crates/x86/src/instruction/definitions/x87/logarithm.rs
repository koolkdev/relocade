//! Logarithmic products replace ST(1) and pop only after the arithmetic response.

use super::*;
use crate::{
    execution::{X87Operand, X87Operands},
    state::x87::X87Specialization,
    x87::LogarithmArgument,
};

instruction_families! {
    FYL2X {
        execute: logarithm_product(LogarithmArgument::Value);
        forms { 0xD9 @ 0xF1 => no_operands(); }
    }
    FYL2XP1 {
        execute: logarithm_product(LogarithmArgument::Increment);
        forms { 0xD9 @ 0xF9 => no_operands(); }
    }
}

fn logarithm_product(
    execution: &mut ExecutionBuilder<'_, '_>,
    argument: LogarithmArgument,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let X87Operands {
        values: operands,
        mut stack_fault,
        ..
    } = execution.read_x87_operands(1.into(), X87Operand::Register(0.into()))?;
    let rounding = execution.x87().rounding()?;
    execution.specialize_x87(X87Specialization::Rounding)?;
    execution.specialize_x87_operands(
        &mut stack_fault,
        operands.logarithm_operands_valid(argument),
    )?;
    let mut result =
        execution.calculate(|body| operands.logarithm_product(body, argument, &rounding))?;
    execution.specialize(|execution| {
        result = execution.admit_x87_candidate(result.in_range.clone())?;
        Ok(())
    })?;
    execution.record_x87_instruction()?;
    result::write_arithmetic_result(execution, 1.into(), result, &stack_fault, true)
}

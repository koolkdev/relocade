//! FPATAN uses quadrant-aware numerical semantics and the shared popping response.

use super::*;
use crate::{
    execution::{X87Operand, X87Operands},
    state::x87::X87Specialization,
};

instruction_families! {
    FPATAN {
        execute: arctangent;
        requires: [Fpu];
        forms { 0xD9 @ 0xF3 => no_operands(); }
    }
}

fn arctangent(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let X87Operands {
        values: operands,
        mut stack_fault,
        ..
    } = execution.read_x87_operands(1.into(), X87Operand::Register(0.into()))?;
    let rounding = execution.x87().rounding()?;
    execution.specialize_x87(X87Specialization::Rounding)?;
    execution.specialize_x87_operands(&mut stack_fault, operands.normal_or_zero())?;
    let mut result = execution.calculate(|body| operands.arctangent(body, &rounding))?;
    execution.specialize(|execution| {
        result = execution.admit_x87_candidate(result.in_range.clone())?;
        Ok(())
    })?;
    execution.record_x87_instruction()?;
    result::write_results(
        execution,
        [result],
        &stack_fault,
        result::Destination::Register {
            index: 1.into(),
            pop: true,
        },
    )
}

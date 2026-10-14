//! FSQRT replaces ST(0), applying the selected precision and rounding modes.

use super::*;
use crate::{
    state::x87::X87Specialization,
    x87::{ArithmeticResult, Operand},
};

instruction_families! {
    FSQRT {
        execute: square_root();
        forms { 0xD9 @ 0xFA => no_operands(); }
    }
}

fn square_root(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let top = execution.x87().read_stack(0)?;
    let mut empty = top.is_empty();
    let operand = Operand::new(&top.value);
    let precision = execution.x87().precision()?;
    let rounding = execution.x87().rounding()?;
    let specialized = execution.specialize(|execution| {
        let controls = execution.specialize_x87(X87Specialization::Arithmetic)?;
        execution.specialize_x87_operands(
            &mut empty,
            operand.normal_or_zero().and(
                top.value
                    .negative()
                    .eq(false)
                    .or(top.value.classification().zero()),
            ),
        )?;
        let rounded = execution.calculate(|body| {
            operand.square_root_candidate(body, precision.clone(), &rounding, controls.nearest_53())
        })?;
        Ok(ArithmeticResult::from_rounding(rounded))
    })?;
    let result = match specialized {
        Some(result) => result,
        None => execution.calculate(|body| operand.square_root(body, precision, &rounding))?,
    };
    execution.record_x87_instruction()?;
    result::write_arithmetic_result(execution, 0.into(), result, &empty, false)
}

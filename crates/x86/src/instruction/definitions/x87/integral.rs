//! FRNDINT rounds ST(0) to an integral floating value under RC alone.

use super::*;
use crate::{
    state::x87::X87Specialization,
    x87::{ArithmeticResult, Operand},
};

instruction_families! {
    FRNDINT {
        execute: round_integral();
        requires: [Fpu];
        forms { 0xD9 @ 0xFC => no_operands(); }
    }
}

fn round_integral(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let top = execution.x87().read_stack(0)?;
    let mut empty = top.is_empty();
    let operand = Operand::new(&top.value);
    let rounding = execution.x87().rounding()?;
    execution.specialize_x87(X87Specialization::Rounding)?;
    let specialized = execution.specialize_x87_operands(&mut empty, operand.normal_or_zero())?;
    let result = operand.round_integral(&rounding);
    let result = if specialized {
        // Integral rounding cannot overflow or underflow. Operand admission
        // therefore proves that only the candidate's precision response remains.
        ArithmeticResult::from_rounding(result.in_range.rounded)
    } else {
        result
    };
    execution.record_x87_instruction()?;
    result::write_results(
        execution,
        [result],
        &empty,
        result::Destination::Register {
            index: 0.into(),
            pop: false,
        },
    )
}

//! Sign operations preserve the magnitude and encoding of a nonempty ST(0).

use super::*;
use crate::{
    state::x87::{ConditionCode, StackValue},
    x87::SignOperation,
};

instruction_families! {
    FCHS {
        execute: change_sign(SignOperation::Negate);
        forms { 0xD9 @ 0xE0 => no_operands(); }
    }
    FABS {
        execute: change_sign(SignOperation::Absolute);
        forms { 0xD9 @ 0xE1 => no_operands(); }
    }
}

fn change_sign(
    execution: &mut ExecutionBuilder<'_, '_>,
    operation: SignOperation,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    let top = execution.x87().read_stack(0)?;
    let empty = top.is_empty();
    let mut x87 = execution.x87();
    let enabled = x87.stack_underflow(&empty)?;
    x87.set_condition_code(ConditionCode::C1, false)?;
    // Masked underflow supplies negative indefinite, regardless of the sign
    // operation or the payload left in the empty slot.
    let result = top.value.change_sign(operation).or_indefinite(&empty);
    x87.write_stack(0, &StackValue::from_value(result), &enabled)
}

//! Stack operations address ST(i) relative to TOP on entry.

use super::*;
use crate::{
    instruction::X87StackIndex,
    state::x87::{C1Update, StackValue},
};

instruction_families! {
    FXCH {
        execute: exchange_register;
        forms { 0xD9 @ 0xC8 + rm => operands(st); }
    }
    FFREE {
        execute: free_register;
        forms { 0xDD @ 0xC0 + rm => operands(st); }
    }
    FINCSTP {
        execute: rotate_stack(true);
        forms { 0xD9 @ 0xF7 => no_operands(); }
    }
    FDECSTP {
        execute: rotate_stack(false);
        forms { 0xD9 @ 0xF6 => no_operands(); }
    }
}

fn exchange_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    other: X87StackIndex,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    let index = other.offset();
    let top = execution.x87().read_stack(0)?;
    let other = execution.x87().read_stack(&index)?;
    let fault = top.is_empty().or(other.is_empty());
    let enabled = execution.x87().stack_underflow(&fault, C1Update::Clear)?;
    // Each empty source is replaced before exchanging; the live source survives.
    execution.x87().write_stack(
        0,
        &StackValue::from_value(other.value.or_indefinite(&other.is_empty())),
        &enabled,
    )?;
    execution.x87().write_stack(
        index,
        &StackValue::from_value(top.value.or_indefinite(&top.is_empty())),
        &enabled,
    )
}

fn free_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    register: X87StackIndex,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    execution.x87().free(register.offset())
}

fn rotate_stack(
    execution: &mut ExecutionBuilder<'_, '_>,
    increment: bool,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    execution.x87().rotate(increment)
}

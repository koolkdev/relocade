//! x87 conditional moves consume integer conditions without changing EFLAGS.

use super::*;
use crate::{
    instruction::X87StackIndex,
    state::x87::{ConditionCode, StackValue},
};

instruction_families! {
    FCMOVB {
        execute: move_register(Condition::B);
        forms { 0xDA @ 0xC0 + rm => operands(st); }
    }
    FCMOVE {
        execute: move_register(Condition::E);
        forms { 0xDA @ 0xC8 + rm => operands(st); }
    }
    FCMOVBE {
        execute: move_register(Condition::BE);
        forms { 0xDA @ 0xD0 + rm => operands(st); }
    }
    FCMOVU {
        execute: move_register(Condition::P);
        forms { 0xDA @ 0xD8 + rm => operands(st); }
    }
    FCMOVNB {
        execute: move_register(Condition::AE);
        forms { 0xDB @ 0xC0 + rm => operands(st); }
    }
    FCMOVNE {
        execute: move_register(Condition::NE);
        forms { 0xDB @ 0xC8 + rm => operands(st); }
    }
    FCMOVNBE {
        execute: move_register(Condition::A);
        forms { 0xDB @ 0xD0 + rm => operands(st); }
    }
    FCMOVNU {
        execute: move_register(Condition::NP);
        forms { 0xDB @ 0xD8 + rm => operands(st); }
    }
}

fn move_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
    condition: Condition,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let condition = execution.condition(condition)?;
    // Intel AP-526 section 4.2.2.1.2 requires both stack operands even
    // when the condition is false, and leaves C1 unchanged without #IS.
    // Read the dynamic source first: runtime decoding may rebase register
    // tracking when it first sees that index.
    let source = execution.x87().read_stack(source.offset())?;
    let previous = execution.x87().read_stack(0)?;
    let value = source.value.select(&condition, &previous.value);
    let tag = condition.select(source.value.classification().tag(), &previous.tag);
    let mut stack_fault = previous.is_empty().or(source.is_empty());
    execution.specialize_without([&mut stack_fault])?;
    execution.record_x87_instruction()?;

    let mut x87 = execution.x87();
    let enabled = x87.stack_underflow(&stack_fault)?;
    x87.set_condition_code_when(ConditionCode::C1, false, &stack_fault)?;
    let value = value.or_indefinite(&stack_fault);
    let tag = stack_fault.select(2_u32, tag);
    // The ordinary condition selects a value, not a discardable exception
    // write guard: a later waiting instruction must retain an untaken move.
    x87.write_stack(0, &StackValue { value, tag }, &enabled)
}

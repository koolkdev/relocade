//! Software interrupt entry through the canonical real-mode vector table.

use super::*;
use crate::{flags::Flag, ExceptionVector};

instruction_families! {
    INT {
        execute: interrupt;
        effects: [control_transfer];
        forms { 0xCD => byte(imm8); }
    }
    INT3 {
        execute: interrupt;
        effects: [control_transfer];
        forms { 0xCC => byte(constant(ExceptionVector::Breakpoint as u32)); }
    }
    INTO {
        execute: overflow_interrupt;
        effects: [control_transfer];
        forms { 0xCE => no_operands(); }
    }
}

fn interrupt(
    execution: &mut ExecutionBuilder<'_, '_>,
    vector: Input<I8>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    let real = execution.real_mode()?;
    execution.unsupported_if(real.eq(false))?;
    let vector = vector.read(execution)?;
    execution.enter_interrupt(vector, fallthrough)
}

fn overflow_interrupt(
    execution: &mut ExecutionBuilder<'_, '_>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    let overflow = execution.read_flag(Flag::OF)?;
    execution.dispatch_if(overflow.eq(false), fallthrough.clone())?;
    let real = execution.real_mode()?;
    execution.unsupported_if(real.eq(false))?;
    execution.enter_interrupt((ExceptionVector::Overflow as u32).into(), fallthrough)
}

//! Software interrupt entry through the canonical real-mode vector table.

use super::*;
use crate::{flags::Flag, ExceptionVector, ExecutionProfile};

instruction_families! {
    INT {
        execute: interrupt(0xcd);
        effects: [control_transfer];
        forms { 0xCD => byte(imm8); }
    }
    INT3 {
        execute: interrupt(0xcc);
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
    opcode: u8,
) -> Result<Val<I32>, BuildError> {
    match execution.profile() {
        ExecutionProfile::Protected(_) => {
            execution.unsupported(u32::from(opcode))?;
            Ok(fallthrough)
        }
        ExecutionProfile::Real16 => {
            let vector = vector.read(execution)?;
            execution.enter_interrupt(vector, fallthrough)
        }
    }
}

fn overflow_interrupt(
    execution: &mut ExecutionBuilder<'_, '_>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    match execution.profile() {
        ExecutionProfile::Protected(_) => execution.unsupported(0xce)?,
        ExecutionProfile::Real16 => {
            let overflow = execution.read_flag(Flag::OF)?;
            execution.enter_interrupt_if(
                overflow,
                (ExceptionVector::Overflow as u32).into(),
                fallthrough.clone(),
            )?;
        }
    }
    Ok(fallthrough)
}

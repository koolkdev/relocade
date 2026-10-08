//! Software interrupt entry through the canonical real-mode vector table.

use super::*;
use crate::{flags::Flag, ExecutionProfile};

instruction_families! {
    INT {
        execute: interrupt(0xcd);
        effects: [memory_read, memory_write, control_transfer, segment_load];
        forms { 0xCD => byte(imm8); }
    }
    INT3 {
        execute: interrupt(0xcc);
        effects: [memory_read, memory_write, control_transfer, segment_load];
        forms { 0xCC => byte(constant(3)); }
    }
    INTO {
        execute: overflow_interrupt;
        effects: [memory_read, memory_write, control_transfer, segment_load];
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
            execution.enter_real_mode_interrupt(vector, fallthrough)
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
            execution.dispatch_if(overflow, |taken| {
                taken.enter_real_mode_interrupt(4.into(), fallthrough.clone())
            })?;
        }
    }
    Ok(fallthrough)
}

//! Software interrupt entry through the canonical real-mode vector table.

use super::*;
use crate::{execution::CodeTarget, flags::image, flags::Flag, ExecutionProfile, Segment};

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
    execution.interpret_tracked_memory()?;
    match execution.profile() {
        ExecutionProfile::Protected(_) => {
            execution.unsupported(u32::from(opcode))?;
            Ok(fallthrough)
        }
        ExecutionProfile::Real16 => {
            let vector = vector.read(execution)?;
            enter_interrupt(execution, vector, fallthrough)
        }
    }
}

fn overflow_interrupt(
    execution: &mut ExecutionBuilder<'_, '_>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    execution.interpret_tracked_memory()?;
    match execution.profile() {
        ExecutionProfile::Protected(_) => execution.unsupported(0xce)?,
        ExecutionProfile::Real16 => {
            let overflow = execution.read_flag(Flag::OF)?;
            execution.dispatch_if(overflow, |taken| {
                enter_interrupt(taken, 4.into(), fallthrough.clone())
            })?;
        }
    }
    Ok(fallthrough)
}

fn enter_interrupt(
    execution: &mut ExecutionBuilder<'_, '_>,
    vector: Val<I8>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    // Real-mode INT always saves three words, including with a 66 prefix.
    let frame = execution.push_frame(6, 6)?;
    let flags_slot = frame.field::<I16>(execution, 4)?;
    let cs_slot = frame.field::<I16>(execution, 2)?;
    let ip_slot = frame.field::<I16>(execution, 0)?;
    let flags = image::read_stack_image::<I16>(execution)?;
    let cs = execution.read_segment_selector(Segment::Cs)?;
    flags_slot.write(execution, &flags)?;
    execution.write_flag(Flag::IF, false)?;
    execution.write_flag(Flag::TF, false)?;
    execution.write_flag(Flag::AC, false)?;
    cs_slot.write(execution, &cs)?;
    ip_slot.write(execution, &fallthrough.truncate::<I16>())?;

    // Real16 uses the conventional 256-entry IVT at linear address zero.
    // Read after the pushes: the stack may alias the vector table.
    let address = vector.unsigned().extend::<I32>().shl(2);
    let selector = execution.read_linear_memory::<I16>(address.add(2))?;
    let offset = execution.read_linear_memory::<I16>(address)?;
    let target = CodeTarget::resolve(execution, offset, &selector)?;
    target.check_limit(execution)?;
    frame.commit(execution, 0)?;
    target.commit(execution)
}

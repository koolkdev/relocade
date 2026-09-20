//! Direct code-segment transfers end the current execution entry.

use super::*;
use crate::{address::MemoryAddress, register::RegisterType};

instruction_families! {
    JMP_FAR_IMMEDIATE {
        execute: jump_far_immediate::<_>;
        effects: [control_transfer, segment_load];
        forms {
            0xEA => word_or_dword(imm, imm16);
        }
    }
    JMP_FAR_INDIRECT {
        execute: jump_far_indirect::<_>;
        effects: [control_transfer, segment_load];
        forms {
            0xFF /5 => word_or_dword(mem);
        }
    }
    CALL_FAR_IMMEDIATE {
        execute: call_far_immediate::<_>;
        effects: [memory_write, control_transfer, segment_load];
        forms {
            0x9A => word_or_dword(imm, imm16);
        }
    }
    CALL_FAR_INDIRECT {
        execute: call_far_indirect::<_>;
        effects: [memory_write, control_transfer, segment_load];
        forms {
            0xFF /3 => word_or_dword(mem);
        }
    }
    RET_FAR {
        execute: return_far::<_>;
        effects: [memory_read, control_transfer, segment_load];
        forms {
            0xCB => word_or_dword(constant(0));
            0xCA => word_or_dword(imm16);
        }
    }
}

fn jump_far_immediate<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    offset: Input<T>,
    selector: Input<I16>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let offset = offset.read(execution)?;
    let selector = selector.read(execution)?;
    execution.jump_far(offset, selector)
}

fn jump_far_indirect<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: MemoryAddress<Val<I32>>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let (offset, selector) = execution.read_far_pointer::<T>(source)?;
    execution.jump_far(offset, selector)
}

fn call_far_immediate<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    offset: Input<T>,
    selector: Input<I16>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let offset = offset.read(execution)?;
    let selector = selector.read(execution)?;
    execution.call_far(offset, selector, fallthrough)
}

fn call_far_indirect<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: MemoryAddress<Val<I32>>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let (offset, selector) = execution.read_far_pointer::<T>(source)?;
    execution.call_far(offset, selector, fallthrough)
}

fn return_far<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    discard_bytes: Input<I16>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let discard_bytes = discard_bytes.read(execution)?;
    execution.return_far::<T>(discard_bytes)
}

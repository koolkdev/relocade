//! Near jumps, calls and returns select the next execution entry.

use super::*;
use crate::{instruction::Operand, register::RegisterType};

instruction_families! {
    JMP_RELATIVE {
        execute: jump_relative;
        effects: [control_transfer];
        forms {
            0xEB => word_or_dword(rel8);
            0xE9 => word_or_dword(rel);
        }
    }
    JCC {
        execute: jump_relative;
        effects: [control_transfer];
        forms {
            0x70 +cc => word_or_dword(rel8);
            0x0F 0x80 +cc => word_or_dword(rel);
        }
    }
    CALL_RELATIVE {
        execute: call_relative;
        effects: [stack_write, control_transfer];
        forms {
            0xE8 => word_or_dword(rel);
        }
    }
    CALL_INDIRECT {
        execute: call_indirect;
        effects: [stack_write, control_transfer];
        forms {
            0xFF /2 => word_or_dword(rm);
        }
    }
    RET {
        execute: return_near;
        effects: [stack_read, control_transfer];
        forms {
            0xC3 => word_or_dword(constant(0));
            0xC2 => word_or_dword(imm16);
        }
    }
    JMP_INDIRECT {
        execute: jump_indirect;
        effects: [control_transfer];
        forms {
            0xFF /4 => word_or_dword(rm);
        }
    }
}

fn jump_relative<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    displacement: Operand<Val<I32>>,
    condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let displacement = Input::<I32>::new(displacement).read(execution)?;
    let target = relative_target::<T>(&fallthrough, displacement);
    Ok(match condition {
        Some(condition) => execution.condition(condition)?.select(target, fallthrough),
        None => target,
    })
}

fn relative_target<T: RegisterType>(fallthrough: &Val<I32>, displacement: Val<I32>) -> Val<I32>
where
    I32: AtLeast<T>,
{
    fallthrough
        .add(displacement)
        .truncate::<T>()
        .unsigned()
        .extend::<I32>()
}

fn call_relative<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    displacement: Operand<Val<I32>>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let displacement = Input::<I32>::new(displacement).read(execution)?;
    let target = relative_target::<T>(&fallthrough, displacement);
    execution.push(fallthrough.truncate::<T>())?;
    Ok(target)
}

fn call_indirect<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Operand<Val<I32>>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    // The target observes entry registers and memory before the return-address push.
    let target = Input::<T>::new(source).read(execution)?;
    execution.push(fallthrough.truncate::<T>())?;
    Ok(target.unsigned().extend::<I32>())
}

fn jump_indirect<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Operand<Val<I32>>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let target = Input::<T>::new(source).read(execution)?;
    Ok(target.unsigned().extend::<I32>())
}

fn return_near<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    discard_bytes: Operand<Val<I32>>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let discard_bytes = Input::<I16>::new(discard_bytes).read(execution)?;
    let target = execution.pop_value::<T>(discard_bytes.unsigned().extend::<I32>())?;
    Ok(target.unsigned().extend::<I32>())
}

//! Near jumps, calls and returns select the next execution entry.

use super::*;
use crate::{
    instruction::Operand,
    register::{Gpr32, RegisterType},
};

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
    JECXZ {
        execute: jump_if_count_zero;
        effects: [control_transfer];
        forms {
            0xE3 => word_or_dword(rel8);
        }
    }
    LOOP {
        execute: loop_relative(None);
        effects: [control_transfer];
        forms {
            0xE2 => word_or_dword(rel8);
        }
    }
    LOOPE {
        execute: loop_relative(Some(Condition::E));
        effects: [control_transfer];
        forms {
            0xE1 => word_or_dword(rel8);
        }
    }
    LOOPNE {
        execute: loop_relative(Some(Condition::NE));
        effects: [control_transfer];
        forms {
            0xE0 => word_or_dword(rel8);
        }
    }
    CALL_RELATIVE {
        execute: call_relative;
        effects: [memory_write, control_transfer];
        forms {
            0xE8 => word_or_dword(rel);
        }
    }
    CALL_INDIRECT {
        execute: call_indirect;
        effects: [memory_write, control_transfer];
        forms {
            0xFF /2 => word_or_dword(rm);
        }
    }
    RET {
        execute: return_near;
        effects: [memory_read, control_transfer];
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
    match condition {
        Some(condition) => {
            let taken = execution.condition(condition)?;
            execution.branch(taken, target, fallthrough)
        }
        None => execution.jump(target),
    }
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

fn jump_if_count_zero<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    displacement: Operand<Val<I32>>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let displacement = Input::<I32>::new(displacement).read(execution)?;
    let count = TypedLocation::<I32>::register(Gpr32::Ecx).read(execution)?;
    let target = relative_target::<T>(&fallthrough, displacement);
    execution.branch(count.eq(0), target, fallthrough)
}

fn loop_relative<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    displacement: Operand<Val<I32>>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
    loop_condition: Option<Condition>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let displacement = Input::<I32>::new(displacement).read(execution)?;
    // Address size selects the counter; operand size only narrows a taken target.
    let count = TypedLocation::<I32>::register(Gpr32::Ecx)
        .read(execution)?
        .sub(1);
    let mut taken = count.ne(0);
    if let Some(condition) = loop_condition {
        taken = taken.and(execution.condition(condition)?);
    }
    let target = relative_target::<T>(&fallthrough, displacement);
    let next_eip = execution.branch(taken, target, fallthrough)?;
    TypedLocation::<I32>::register(Gpr32::Ecx).write(execution, count)?;
    Ok(next_eip)
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
    let target = execution.jump(target)?;
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
    let target = execution.jump(target.unsigned().extend::<I32>())?;
    execution.push(fallthrough.truncate::<T>())?;
    Ok(target)
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
    execution.jump(target.unsigned().extend::<I32>())
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
    let pop = execution.read_stack::<T>()?;
    let target = execution.jump(pop.value().unsigned().extend::<I32>())?;
    pop.commit(execution, discard_bytes.unsigned().extend::<I32>())?;
    Ok(target)
}

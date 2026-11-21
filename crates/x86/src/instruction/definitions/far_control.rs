//! Far jumps, calls and returns end the current execution entry.
//!
//! Protected CALL and all returns require complete operand-sized slots in SS.
//! Real CALL checks only the transferred span. Backing checks and transfers cover
//! the values, including two selector bytes; dword selector padding stays untouched.
//! This combines RET's full-slot capacity check with P6 selector-transfer behavior;
//! their descriptions leave the access extent ambiguous. See Intel SDM Volume 3B,
//! section 22.31.1:
//! <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3b-part-2-manual.pdf#page=575>.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::CodeTarget,
    flags::{image, Flag},
    register::RegisterType,
};

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
        effects: [control_transfer, segment_load];
        flow: FarCall;
        forms {
            0x9A => word_or_dword(imm, imm16);
        }
    }
    CALL_FAR_INDIRECT {
        execute: call_far_indirect::<_>;
        effects: [control_transfer, segment_load];
        flow: FarCall;
        forms {
            0xFF /3 => word_or_dword(mem);
        }
    }
    RET_FAR {
        execute: return_far::<_>;
        effects: [control_transfer, segment_load];
        forms {
            0xCB => word_or_dword(constant(0));
            0xCA => word_or_dword(imm16);
        }
    }
    IRET {
        execute: return_interrupt::<_>;
        effects: [control_transfer, segment_load];
        forms { 0xCF => word_or_dword(); }
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
    jump_far(execution, offset, selector)
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
    jump_far(execution, offset, selector)
}

fn jump_far<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    offset: Val<T>,
    selector: Val<I16>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let target = CodeTarget::resolve(execution, offset, &selector)?;
    target.check_limit(execution)?;
    target.commit(execution)
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
    execution.call_far(offset, selector, fallthrough, 0)
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
    execution.call_far(offset, selector, fallthrough, 0)
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
    let frame = execution.pop_frame(2 * T::BYTES, 2 * T::BYTES)?;
    let offset = frame.field::<T>(execution, 0)?.read(execution)?;
    let selector = frame.field::<I16>(execution, T::BYTES)?.read(execution)?;
    let target = resolve_return_target(execution, offset, &selector)?;
    frame.commit(execution, discard_bytes.unsigned().extend::<I32>())?;
    target.commit(execution)
}

fn return_interrupt<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    // Only protected mode interprets entry NT as a task return.
    let protected = execution.protected_mode()?;
    let nested_task = execution.read_flag(Flag::NT)?;
    execution.unsupported_if(protected.and(nested_task), 0xcf)?;
    let frame = execution.pop_frame(3 * T::BYTES, 3 * T::BYTES)?;
    let offset = frame.field::<T>(execution, 0)?.read(execution)?;
    // Real-mode IRETD checks the new offset before reading either CS or FLAGS.
    let real = execution.real_mode()?;
    let offset32 = offset.unsigned().extend::<I32>();
    execution.fault_if(
        real.and(offset32.unsigned().ge(0x1_0000)),
        crate::ExceptionValues::general_protection(0),
    )?;
    let selector = frame.field::<I16>(execution, T::BYTES)?.read(execution)?;
    let flags = frame.field::<T>(execution, 2 * T::BYTES)?.read(execution)?;
    let target = resolve_return_target(execution, offset, &selector)?;
    let enabled = image::write_stack_image(execution, &flags)?;
    frame.commit(execution, 0)?;
    let next_ip = target.commit(execution)?;
    execution.interrupts_enabled_if(enabled, &next_ip)?;
    Ok(next_ip)
}

fn resolve_return_target<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    offset: Val<T>,
    selector: &Val<I16>,
) -> Result<CodeTarget, BuildError>
where
    I32: AtLeast<T>,
{
    // Protected returns cannot go inward. Only same-level returns are admitted.
    let protected = execution.protected_mode()?;
    let cpl = execution.privilege_level()?;
    let rpl = selector.truncate::<I8>().and(3);
    execution.fault_if(
        protected.and(rpl.unsigned().lt(cpl)),
        crate::ExceptionValues::general_protection(selector.unsigned().extend::<I32>().and(0xfffc)),
    )?;
    let target = CodeTarget::resolve(execution, offset, selector)?;
    target.check_limit(execution)?;
    Ok(target)
}

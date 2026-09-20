//! Far jumps, calls and returns end the current execution entry.
//!
//! Frame compatibility policy: all operand-sized slots must fit SS, but paging
//! and transfers cover only their values, including two selector bytes. Dword
//! selector padding stays untouched. This combines RET's full-slot capacity check
//! with P6 selector-transfer behavior; their descriptions leave the access extent
//! ambiguous. See Intel SDM Volume 3B, section 22.31.1:
//! <https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-3b-part-2-manual.pdf#page=575>.

use super::*;
use crate::{
    address::MemoryAddress,
    exception::Exception,
    execution::CodeTarget,
    flags::{image, Flag},
    register::RegisterType,
    Segment,
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
    IRET {
        execute: return_interrupt::<_>;
        effects: [memory_read, control_transfer, segment_load];
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
    call_far(execution, offset, selector, fallthrough)
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
    call_far(execution, offset, selector, fallthrough)
}

fn call_far<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    offset: Val<T>,
    selector: Val<I16>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let target = CodeTarget::resolve(execution, offset, &selector)?;
    let frame = execution.push_frame(2 * T::BYTES, 2 * T::BYTES)?;
    target.check_limit(execution)?;
    // Both slots must fit SS. The selector's unused high word is not touched.
    // Prove both fields in push order before writing either of them.
    let selector_slot = frame.field::<I16>(execution, T::BYTES)?;
    let offset_slot = frame.field::<T>(execution, 0)?;
    let old_cs = execution.read_segment_selector(Segment::Cs)?;
    selector_slot.write(execution, &old_cs)?;
    offset_slot.write(execution, &fallthrough.truncate::<T>())?;
    frame.commit(execution, 0)?;
    target.commit(execution)
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
    // NT selects a task return before stack access. Task switching uses the
    // unsupported-execution exit.
    let nested_task = execution.read_flag(Flag::NT)?;
    execution.unsupported_if(nested_task, 0xcf)?;
    let frame = execution.pop_frame(3 * T::BYTES, 3 * T::BYTES)?;
    let offset = frame.field::<T>(execution, 0)?.read(execution)?;
    let selector = frame.field::<I16>(execution, T::BYTES)?.read(execution)?;
    let flags = frame.field::<T>(execution, 2 * T::BYTES)?.read(execution)?;
    let target = resolve_return_target(execution, offset, &selector)?;
    execution.write_flags(image::stack_change(&flags))?;
    frame.commit(execution, 0)?;
    target.commit(execution)
}

fn resolve_return_target<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    offset: Val<T>,
    selector: &Val<I16>,
) -> Result<CodeTarget, BuildError>
where
    I32: AtLeast<T>,
{
    // Returns cannot go inward. With CPL fixed at 3, only RPL 3 is valid.
    // After this check the direct-CS resolver implements the return policy.
    execution.fault_if(
        selector.and(3).ne(3),
        Exception::GeneralProtection {
            error_code: selector.unsigned().extend::<I32>().and(0xfffc),
        },
    )?;
    let target = CodeTarget::resolve(execution, offset, selector)?;
    target.check_limit(execution)?;
    Ok(target)
}

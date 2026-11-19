mod frame;

use frame::{enter_frame, leave_frame};

use super::*;
use crate::address::RegisterValue;
use crate::flags::image;
use crate::memory::Intent;
use crate::register::{Gpr32, RegisterType};

instruction_families! {
    PUSH {
        execute: push;
        forms {
            0x50 +reg => word_or_dword(opcode_reg);
            0x68 => word_or_dword(imm);
            0x6A => word_or_dword(signed_imm8);
            0xFF /6 => word_or_dword(rm);
        }
    }
    POP {
        execute: pop;
        forms {
            0x58 +reg => word_or_dword(opcode_reg);
            0x8F /0 => word_or_dword(rm);
        }
    }
    PUSHA {
        execute: push_all_registers::<_>;
        forms { 0x60 => word_or_dword(); }
    }
    POPA {
        execute: pop_all_registers::<_>;
        forms { 0x61 => word_or_dword(); }
    }
    PUSHF {
        execute: push_flags::<_>;
        forms {
            0x9C => word_or_dword();
        }
    }
    POPF {
        execute: pop_flags::<_>;
        effects: [control_transfer];
        flow: Fallthrough;
        forms {
            0x9D => word_or_dword();
        }
    }
    ENTER {
        execute: enter_frame::<_>;
        forms { 0xC8 => word_or_dword(imm16, imm8); }
    }
    LEAVE {
        execute: leave_frame::<_>;
        forms { 0xC9 => word_or_dword(); }
    }
}

fn push<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    // The source, including ESP or an ESP-based address, observes entry ESP.
    let value = source.read(execution)?;
    execution.push(value, T::BYTES)
}

fn pop<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
) -> Result<(), BuildError> {
    let frame = execution.pop_frame(T::BYTES, T::BYTES)?;
    let value = frame.field::<T>(execution, 0)?.read(execution)?;
    // Address reads see next ESP while the fault state still holds entry ESP.
    // On a 16-bit stack, this uses the preserved upper ESP word even on wrap.
    // That wrapped destination policy is processor-family-specific.
    let target = destination.prepare_write(
        execution,
        &[RegisterValue {
            register: Gpr32::Esp,
            value: frame.next_pointer().value(),
        }],
    )?;
    // POP ESP overwrites the increment; POP SP preserves its upper word.
    frame.commit(execution, 0)?;
    target.write(execution, value)
}

fn push_all_registers<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let pointer = execution.stack_pointer()?;
    // Real-mode PUSHA/PUSHAD raise #GP at these offsets before the first store.
    let sp = pointer.offset();
    let low_odd = sp
        .and(1)
        .ne(0)
        .and(sp.unsigned().ge(7))
        .and(sp.unsigned().lt(16));
    let protected = execution.protected_mode()?;
    execution.fault_if(
        protected.eq(false).and(low_odd),
        crate::Exception::GeneralProtection { error_code: 0 },
    )?;
    // Capture entry ESP along with the other registers before committing the frame.
    let values = Gpr32::ALL
        .into_iter()
        .map(|register| TypedLocation::<T>::register(register).read(execution))
        .collect::<Result<Vec<_>, _>>()?;
    pointer.push_values(execution, &values)
}

fn pop_all_registers<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError> {
    let pointer = execution.stack_pointer()?;
    let mut stack = pointer.check_access::<T>(execution, 8, Intent::Read)?;
    execution.guard_accesses(true, [&mut stack.access])?;
    for (index, register) in Gpr32::ALL.into_iter().rev().enumerate() {
        // The discarded SP/ESP slot is checked without reading it.
        let field = stack.field::<T>(execution, index as u32)?;
        if register != Gpr32::Esp {
            let value = field.read(execution)?;
            // Earlier restores survive a later fault, while ESP stays at entry.
            TypedLocation::<T>::register(register).write(execution, value)?;
        }
    }
    pointer.advance(8 * T::BYTES).commit(execution)
}

fn push_flags<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let image = image::read_stack_image::<T>(execution)?;
    execution.push(image, T::BYTES)
}

fn pop_flags<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let frame = execution.pop_frame(T::BYTES, T::BYTES)?;
    let flags = frame.field::<T>(execution, 0)?.read(execution)?;
    frame.commit(execution, 0)?;
    let enabled = image::write_stack_image(execution, &flags)?;
    execution.interrupts_enabled_if(enabled, &fallthrough)?;
    Ok(fallthrough)
}

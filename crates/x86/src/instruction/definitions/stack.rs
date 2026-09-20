use super::*;
use crate::address::RegisterValue;
use crate::flags::image;
use crate::register::{Gpr32, RegisterType};

instruction_families! {
    PUSH {
        execute: push;
        effects: [memory_write];
        forms {
            0x50 +reg => word_or_dword(opcode_reg);
            0x68 => word_or_dword(imm);
            0x6A => word_or_dword(signed_imm8);
            0xFF /6 => word_or_dword(rm);
        }
    }
    POP {
        execute: pop;
        effects: [memory_read];
        forms {
            0x58 +reg => word_or_dword(opcode_reg);
            0x8F /0 => word_or_dword(rm);
        }
    }
    PUSHA {
        execute: push_all_registers::<_>;
        effects: [memory_write];
        forms { 0x60 => word_or_dword(); }
    }
    POPA {
        execute: pop_all_registers::<_>;
        effects: [memory_read];
        forms { 0x61 => word_or_dword(); }
    }
    PUSHF {
        execute: push_flags::<_>;
        effects: [memory_write];
        forms {
            0x9C => word_or_dword();
        }
    }
    POPF {
        execute: pop_flags::<_>;
        effects: [memory_read];
        forms {
            0x9D => word_or_dword();
        }
    }
    ENTER {
        execute: enter_frame::<_>;
        effects: [memory_read, memory_write];
        forms { 0xC8 => word_or_dword(imm16, imm8); }
    }
    LEAVE {
        execute: leave_frame::<_>;
        effects: [memory_read];
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
    let mut pointer = execution.stack_pointer()?;
    for register in Gpr32::ALL {
        // ESP stays at its entry value until all eight pushes succeed.
        let value = TypedLocation::<T>::register(register).read(execution)?;
        let frame = pointer.push_frame(execution, T::BYTES, T::BYTES)?;
        frame.field::<T>(execution, 0)?.write(execution, &value)?;
        pointer = frame.next_pointer();
    }
    pointer.commit(execution)
}

fn pop_all_registers<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError> {
    let mut pointer = execution.stack_pointer()?;
    for register in Gpr32::ALL.into_iter().rev() {
        let frame = pointer.pop_frame(execution, T::BYTES, T::BYTES)?;
        // The discarded SP/ESP slot still requires segment and page checks.
        let field = frame.field::<T>(execution, 0)?;
        if register != Gpr32::Esp {
            let value = field.read(execution)?;
            // Earlier restores survive a later fault, while ESP stays at entry.
            TypedLocation::<T>::register(register).write(execution, value)?;
        }
        pointer = frame.next_pointer();
    }
    pointer.commit(execution)
}

fn push_flags<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let image = match T::BYTES {
        2 => image::WORD.pack(execution.read_flags(image::WORD.flags())?),
        4 => image::DWORD.pack(execution.read_flags(image::DWORD.flags())?),
        _ => unreachable!("stack flag images use word or dword operands"),
    };
    execution.push(image.truncate::<T>(), T::BYTES)
}

fn pop_flags<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let frame = execution.pop_frame(T::BYTES, T::BYTES)?;
    let flags = frame.field::<T>(execution, 0)?.read(execution)?;
    frame.commit(execution, 0)?;
    execution.write_flags(image::stack_change(&flags))
}

fn leave_frame<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError> {
    let frame_pointer = TypedLocation::<I32>::register(Gpr32::Ebp).read(execution)?;
    let pointer = execution.stack_pointer()?.with_offset(frame_pointer);
    // SS.B selects SP/ESP independently of the popped BP/EBP width. Keep
    // the replacement prospective until the frame read has succeeded.
    let frame = pointer.pop_frame(execution, T::BYTES, T::BYTES)?;
    let saved_frame_pointer = frame.field::<T>(execution, 0)?.read(execution)?;
    frame.commit(execution, 0)?;
    TypedLocation::<T>::register(Gpr32::Ebp).write(execution, saved_frame_pointer)
}

fn enter_frame<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    allocation: Input<I16>,
    nesting: Input<I8>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let allocation = allocation.read(execution)?;
    let nesting = nesting.read(execution)?;
    execution.enter_frame::<T>(allocation, nesting)
}

use super::*;
use crate::flags::image;
use crate::register::RegisterType;

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
    PUSHF {
        execute: push_flags;
        effects: [memory_write];
        forms {
            0x9C => word_or_dword();
        }
    }
    POPF {
        execute: pop_flags;
        effects: [memory_read];
        forms {
            0x9D => word_or_dword();
        }
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
    execution.push(value)
}

fn pop<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
) -> Result<(), BuildError> {
    execution.pop::<T>(destination.into_location())
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
    execution.push(image.truncate::<T>())
}

fn pop_flags<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let image = execution.pop_value::<T>(0)?.unsigned().extend::<I32>();
    let change = match T::BYTES {
        2 => image::WORD.change(&image),
        4 => image::DWORD.change(&image),
        _ => unreachable!("stack flag images use word or dword operands"),
    };
    execution.write_flags(change)
}

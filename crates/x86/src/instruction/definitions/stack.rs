use super::*;
use crate::register::RegisterType;

instruction_families! {
    PUSH {
        execute: push;
        effects: [stack_write];
        forms {
            0x50 +reg => word_or_dword(opcode_reg);
            0x68 => word_or_dword(imm);
            0x6A => word_or_dword(signed_imm8);
            0xFF /6 => word_or_dword(rm);
        }
    }
    POP {
        execute: pop;
        effects: [stack_read];
        forms {
            0x58 +reg => word_or_dword(opcode_reg);
            0x8F /0 => word_or_dword(rm);
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

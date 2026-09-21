use super::*;
use crate::register::{Gpr32, RegisterType};

instruction_families! {
    MOV {
        execute: mov;
        forms {
            0x88 => byte(rm, modrm_reg);
            0x89 => word_or_dword(rm, modrm_reg);
            0x8A => byte(modrm_reg, rm);
            0x8B => word_or_dword(modrm_reg, rm);
            0xB0 +reg => byte(opcode_reg, imm8);
            0xB8 +reg => word_or_dword(opcode_reg, imm);
            0xC6 /0 => byte(rm, imm8);
            0xC7 /0 => word_or_dword(rm, imm);
            0xA0 => byte(AL, moffs);
            0xA1 => word_or_dword(accumulator, moffs);
            0xA2 => byte(moffs, AL);
            0xA3 => word_or_dword(moffs, accumulator);
        }
    }
    LEA {
        execute: mov;
        forms {
            0x8D => word_or_dword(modrm_reg, address);
        }
    }
    XLAT {
        execute: translate_byte;
        effects: [memory_read];
        forms {
            0xD7 => no_operands();
        }
    }
}

fn translate_byte(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let index = TypedLocation::<I8>::register(Gpr32::Eax)
        .read(execution)?
        .unsigned()
        .extend::<I32>();
    let value = execution
        .memory_at_register::<I8>(Gpr32::Ebx, execution.data_segment())
        .offset_memory(index)
        .read(execution)?;
    TypedLocation::<I8>::register(Gpr32::Eax).write(execution, value)
}

fn mov<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let value = source.read(execution)?;
    destination.write(execution, value)
}

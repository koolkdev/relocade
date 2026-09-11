//! Integer extensions into a destination register or an implicit accumulator pair.

use super::*;
use crate::register::RegisterType;

instruction_families! {
    MOVZX {
        execute: movzx;
        forms {
            0x0F 0xB6 => word_or_dword(modrm_reg, rm8);
            0x0F 0xB7 => word_or_dword(modrm_reg, rm16);
        }
    }
    MOVSX {
        execute: movsx;
        forms {
            0x0F 0xBE => word_or_dword(modrm_reg, rm8);
            0x0F 0xBF => word_or_dword(modrm_reg, rm16);
        }
    }
    CBW_CWDE {
        execute: movsx;
        forms {
            0x98 => word(AX, AL) | dword(EAX, AX);
        }
    }
    CWD_CDQ {
        execute: sign_fill;
        forms {
            0x99 => word(DX, AX) | dword(EDX, EAX);
        }
    }
}

fn movzx<Destination: RegisterType + AtLeast<Source>, Source: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<Destination>,
    source: Input<Source>,
) -> Result<(), BuildError>
where
    I32: AtLeast<Source>,
{
    let value = source.read(execution)?;
    destination.write(execution, value.unsigned().extend::<Destination>())
}

fn movsx<Destination: RegisterType + AtLeast<Source>, Source: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<Destination>,
    source: Input<Source>,
) -> Result<(), BuildError>
where
    I32: AtLeast<Source>,
{
    let value = source.read(execution)?;
    destination.write(execution, value.signed().extend::<Destination>())
}

fn sign_fill<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let value = source.read(execution)?;
    destination.write(execution, value.signed().shr(T::BYTES * 8 - 1))
}

//! Integer extensions into a destination register or an implicit accumulator pair.

use super::*;
use crate::register::{Gpr32, RegisterType};

const SIGNED_BYTE: SizedHandlers<Handler> = binary_handlers!(movsx, source = I8, sized);
const SIGNED_WORD: SizedHandlers<Handler> = binary_handlers!(movsx, source = I16, sized);

pub(super) const FORMS: [Form; 6] = [
    register_rm(
        OpcodeMap::Extended,
        0xb6,
        binary_handlers!(movzx, source = I8, sized),
        RegisterSide::Left,
    ),
    register_rm(
        OpcodeMap::Extended,
        0xb7,
        binary_handlers!(movzx, source = I16, sized),
        RegisterSide::Left,
    ),
    register_rm(OpcodeMap::Extended, 0xbe, SIGNED_BYTE, RegisterSide::Left),
    register_rm(OpcodeMap::Extended, 0xbf, SIGNED_WORD, RegisterSide::Left),
    accumulator_form(
        0x98,
        Gpr32::Eax,
        SizedHandlers {
            word: SIGNED_BYTE.word,
            dword: SIGNED_WORD.dword,
        },
    ),
    accumulator_form(0x99, Gpr32::Edx, binary_handlers!(sign_fill).sized),
];

const fn accumulator_form(
    opcode: u8,
    destination: Gpr32,
    handlers: SizedHandlers<Handler>,
) -> Form {
    primary_form(
        opcode,
        Encoding::OpcodeOnly,
        handlers,
        OperandBindingShape::Binary {
            left: LocationBinding::FixedRegister(destination),
            right: OperandBinding::Location(LocationBinding::FixedRegister(Gpr32::Eax)),
        },
    )
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

use super::*;
use crate::register::RegisterType;

const HANDLERS: IntegerHandlers<Handler> = binary_handlers!(mov);

const fn opcode_immediate(
    opcode: u8,
    immediate: ImmediateWidth,
    handlers: SizedHandlers<Handler>,
) -> Form {
    let mut form = primary_form(
        opcode,
        Encoding::OpcodeRegisterImmediate { immediate },
        handlers,
        OperandBindingShape::Binary {
            left: LocationBinding::Register,
            right: OperandBinding::Immediate,
        },
    );
    form.mask = 0xf8;
    form
}

const OPCODE_REGISTER_IMMEDIATE_FORMS: [Form; 2] = [
    opcode_immediate(0xb8, ImmediateWidth::OperandSize, HANDLERS.sized),
    opcode_immediate(
        0xb0,
        ImmediateWidth::Byte,
        SizedHandlers::fixed(HANDLERS.byte),
    ),
];

const MOV_MODRM_FORMS: [Form; 6] = [
    register_rm(0x89, HANDLERS.sized, RegisterRole::Source),
    register_rm(0x8b, HANDLERS.sized, RegisterRole::Destination),
    register_rm(
        0x88,
        SizedHandlers::fixed(HANDLERS.byte),
        RegisterRole::Source,
    ),
    register_rm(
        0x8a,
        SizedHandlers::fixed(HANDLERS.byte),
        RegisterRole::Destination,
    ),
    rm_immediate(
        0xc6,
        0,
        ImmediateWidth::Byte,
        SizedHandlers::fixed(HANDLERS.byte),
    ),
    rm_immediate(0xc7, 0, ImmediateWidth::OperandSize, HANDLERS.sized),
];

const fn accumulator_offset(
    opcode: u8,
    handlers: SizedHandlers<Handler>,
    destination: LocationBinding,
    source: LocationBinding,
) -> Form {
    primary_form(
        opcode,
        Encoding::AccumulatorOffset,
        handlers,
        OperandBindingShape::Binary {
            left: destination,
            right: OperandBinding::Location(source),
        },
    )
}

const ACCUMULATOR_OFFSET_FORMS: [Form; 4] = [
    accumulator_offset(
        0xa0,
        SizedHandlers::fixed(HANDLERS.byte),
        LocationBinding::Accumulator,
        LocationBinding::AbsoluteOffset,
    ),
    accumulator_offset(
        0xa1,
        HANDLERS.sized,
        LocationBinding::Accumulator,
        LocationBinding::AbsoluteOffset,
    ),
    accumulator_offset(
        0xa2,
        SizedHandlers::fixed(HANDLERS.byte),
        LocationBinding::AbsoluteOffset,
        LocationBinding::Accumulator,
    ),
    accumulator_offset(
        0xa3,
        HANDLERS.sized,
        LocationBinding::AbsoluteOffset,
        LocationBinding::Accumulator,
    ),
];

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    OPCODE_REGISTER_IMMEDIATE_FORMS
        .iter()
        .chain(MOV_MODRM_FORMS.iter())
        .chain(ACCUMULATOR_OFFSET_FORMS.iter())
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

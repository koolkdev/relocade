use super::*;
use crate::register::RegisterType;

const HANDLERS: IntegerHandlers<Handler> = binary_handlers!(xchg, right = TypedLocation);

const fn accumulator_register() -> Form {
    let mut form = primary_form(
        0x90,
        Encoding::OpcodeRegister,
        HANDLERS.sized,
        OperandBindingShape::Binary {
            left: LocationBinding::Accumulator,
            right: OperandBinding::Location(LocationBinding::Register),
        },
    );
    form.mask = 0xf8;
    form
}

pub(super) const FORMS: [Form; 3] = [
    register_rm(
        0x86,
        SizedHandlers::fixed(HANDLERS.byte),
        RegisterSide::Right,
    ),
    register_rm(0x87, HANDLERS.sized, RegisterSide::Right),
    accumulator_register(),
];

fn xchg<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: TypedLocation<T>,
    right: TypedLocation<T>,
) -> Result<(), BuildError> {
    left.exchange(execution, right)
}

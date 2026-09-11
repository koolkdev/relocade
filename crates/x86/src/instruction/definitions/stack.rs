use super::*;
use crate::register::RegisterType;

const PUSH: SizedHandlers<Handler> = unary_handlers!(push, Input, sized);
const POP: SizedHandlers<Handler> = unary_handlers!(pop, TypedLocation, sized);

const fn push_immediate(opcode: u8, immediate: ImmediateWidth) -> Form {
    primary_form(
        opcode,
        Encoding::Immediate { immediate },
        PUSH,
        OperandBindingShape::Unary(OperandBinding::Immediate),
    )
}

const fn stack_forms() -> [Form; 6] {
    let mut forms = [
        opcode_register(0x50, PUSH),
        opcode_register(0x58, POP),
        push_immediate(0x68, ImmediateWidth::OperandSize),
        push_immediate(0x6a, ImmediateWidth::SignedByte),
        rm(0xff, 6, PUSH),
        rm(0x8f, 0, POP),
    ];
    let mut index = 0;
    while index < forms.len() {
        forms[index].implicit_memory = true;
        index += 1;
    }
    forms
}

pub(super) const FORMS: [Form; 6] = stack_forms();

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

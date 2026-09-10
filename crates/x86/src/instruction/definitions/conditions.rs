use super::*;
use crate::register::RegisterType;

const fn set_condition(code: u8) -> Form {
    let mut form = primary_form(
        0x90 + code,
        Encoding::Rm,
        SizedHandlers::fixed(unary_handlers!(setcc, TypedLocation, width = I8, condition)),
        OperandBindingShape::Unary(OperandBinding::Location(LocationBinding::Rm)),
    );
    form.map = OpcodeMap::Extended;
    form.condition = Some(Condition::from_code(code));
    form
}

const fn move_condition(code: u8) -> Form {
    let mut form = register_rm(
        0x40 + code,
        binary_handlers!(cmov, condition).sized,
        RegisterSide::Left,
    );
    form.map = OpcodeMap::Extended;
    form.condition = Some(Condition::from_code(code));
    form
}

const fn condition_forms() -> [Form; 32] {
    let mut forms = [set_condition(0); 32];
    let mut code = 0;
    while code < 16 {
        forms[code] = set_condition(code as u8);
        forms[16 + code] = move_condition(code as u8);
        code += 1;
    }
    forms
}

pub(super) const FORMS: [Form; 32] = condition_forms();

fn setcc(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I8>,
    condition: Condition,
) -> Result<(), BuildError> {
    let value = execution.condition(condition)?;
    destination.write(execution, value.unsigned().extend::<I8>())
}

fn cmov<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
    condition: Condition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    // The source access can fault even when the condition is false.
    let value = source.read(execution)?;
    let predicate = execution.condition(condition)?;
    destination.update(execution, |_, previous| {
        Ok(predicate.select(value, previous))
    })
}

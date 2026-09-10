use super::*;

const fn set_condition(code: u8) -> Form {
    let mut form = primary_form(
        0x90 + code,
        Encoding::Rm,
        SizedHandlers::fixed(Handler::Unary(
            |execution, operand, condition, fallthrough| {
                let condition = condition.expect("SETcc forms bind a condition");
                setcc(
                    execution,
                    TypedLocation::<I8>::from_operand(operand),
                    condition,
                )?;
                Ok(fallthrough)
            },
        )),
        OperandBindingShape::Unary(OperandBinding::Location(LocationBinding::Rm)),
    );
    form.map = OpcodeMap::Extended;
    form.condition = Some(Condition::from_code(code));
    form
}

const fn condition_forms() -> [Form; 16] {
    let mut forms = [set_condition(0); 16];
    let mut code = 0;
    while code < forms.len() {
        forms[code] = set_condition(code as u8);
        code += 1;
    }
    forms
}

pub(super) const FORMS: [Form; 16] = condition_forms();

fn setcc(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I8>,
    condition: Condition,
) -> Result<(), BuildError> {
    let value = execution.condition(condition)?;
    destination.write(execution, value.unsigned().extend::<I8>())
}

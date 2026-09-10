use super::*;

const fn relative(
    opcode: u8,
    map: OpcodeMap,
    immediate: ImmediateWidth,
    condition: Option<Condition>,
) -> Form {
    let mut form = primary_form(
        opcode,
        Encoding::Immediate { immediate },
        SizedHandlers {
            word: Handler::Unary(branch::<I16>),
            dword: Handler::Unary(branch::<I32>),
        },
        OperandBindingShape::Unary(OperandBinding::Immediate),
    );
    form.condition = condition;
    form.ends_block = true;
    form.map = map;
    form
}

const fn branch_forms() -> [Form; 34] {
    let mut forms =
        [const { relative(0xeb, OpcodeMap::Primary, ImmediateWidth::SignedByte, None) }; 34];
    forms[1] = relative(0xe9, OpcodeMap::Primary, ImmediateWidth::OperandSize, None);
    let mut code = 0;
    while code < 16 {
        let condition = Some(Condition::from_code(code as u8));
        forms[2 + code] = relative(
            0x70 + code as u8,
            OpcodeMap::Primary,
            ImmediateWidth::SignedByte,
            condition,
        );
        forms[18 + code] = relative(
            0x80 + code as u8,
            OpcodeMap::Extended,
            ImmediateWidth::OperandSize,
            condition,
        );
        code += 1;
    }
    forms
}

pub(super) const FORMS: [Form; 34] = branch_forms();

fn branch<T: crate::register::RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    displacement: crate::instruction::Operand<Val<I32>>,
    condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let displacement = Input::<I32>::new(displacement).read(execution)?;
    let target = fallthrough
        .add(displacement)
        .truncate::<T>()
        .unsigned()
        .extend::<I32>();
    Ok(match condition {
        Some(condition) => execution.condition(condition)?.select(target, fallthrough),
        None => target,
    })
}

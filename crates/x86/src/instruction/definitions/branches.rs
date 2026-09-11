//! Near jumps, calls and returns select the next execution entry.

use super::*;
use crate::{instruction::Operand, register::RegisterType};

const fn relative_jump_form(
    opcode: u8,
    map: OpcodeMap,
    immediate: ImmediateWidth,
    condition: Option<Condition>,
) -> Form {
    let mut form = primary_form(
        opcode,
        Encoding::Immediate { immediate },
        SizedHandlers {
            word: Handler::Unary(jump_relative::<I16>),
            dword: Handler::Unary(jump_relative::<I32>),
        },
        OperandBindingShape::Unary(OperandBinding::Immediate),
    );
    form.condition = condition;
    form.ends_block = true;
    form.map = map;
    form
}

const fn relative_jump_forms() -> [Form; 34] {
    let mut forms =
        [const { relative_jump_form(0xeb, OpcodeMap::Primary, ImmediateWidth::SignedByte, None) };
            34];
    forms[1] = relative_jump_form(0xe9, OpcodeMap::Primary, ImmediateWidth::OperandSize, None);
    let mut code = 0;
    while code < 16 {
        let condition = Some(Condition::from_code(code as u8));
        forms[2 + code] = relative_jump_form(
            0x70 + code as u8,
            OpcodeMap::Primary,
            ImmediateWidth::SignedByte,
            condition,
        );
        forms[18 + code] = relative_jump_form(
            0x80 + code as u8,
            OpcodeMap::Extended,
            ImmediateWidth::OperandSize,
            condition,
        );
        code += 1;
    }
    forms
}

const RELATIVE_JUMPS: [Form; 34] = relative_jump_forms();

const CALL_RELATIVE: SizedHandlers<Handler> = SizedHandlers {
    word: Handler::Unary(call_relative::<I16>),
    dword: Handler::Unary(call_relative::<I32>),
};
const CALL_INDIRECT: SizedHandlers<Handler> = SizedHandlers {
    word: Handler::Unary(call_indirect::<I16>),
    dword: Handler::Unary(call_indirect::<I32>),
};
const RETURN: SizedHandlers<Handler> = SizedHandlers {
    word: Handler::Unary(return_near::<I16>),
    dword: Handler::Unary(return_near::<I32>),
};

const fn stack_transfer(mut form: Form) -> Form {
    form.implicit_memory = true;
    form.ends_block = true;
    form
}

const CALLS: [Form; 2] = [
    stack_transfer(primary_form(
        0xe8,
        Encoding::Immediate {
            immediate: ImmediateWidth::OperandSize,
        },
        CALL_RELATIVE,
        OperandBindingShape::Unary(OperandBinding::Immediate),
    )),
    stack_transfer(rm(0xff, 2, CALL_INDIRECT)),
];

const RETURNS: [Form; 2] = [
    stack_transfer(primary_form(
        0xc3,
        Encoding::OpcodeOnly,
        RETURN,
        OperandBindingShape::Unary(OperandBinding::Constant(0)),
    )),
    stack_transfer(primary_form(
        0xc2,
        Encoding::Immediate {
            immediate: ImmediateWidth::Word,
        },
        RETURN,
        OperandBindingShape::Unary(OperandBinding::Immediate),
    )),
];

const INDIRECT_JUMP: Form = {
    let mut form = rm(
        0xff,
        4,
        SizedHandlers {
            word: Handler::Unary(jump_indirect::<I16>),
            dword: Handler::Unary(jump_indirect::<I32>),
        },
    );
    form.ends_block = true;
    form
};

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    RELATIVE_JUMPS
        .iter()
        .chain(CALLS.iter())
        .chain(RETURNS.iter())
        .chain(std::iter::once(&INDIRECT_JUMP))
}

fn jump_relative<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    displacement: Operand<Val<I32>>,
    condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let displacement = Input::<I32>::new(displacement).read(execution)?;
    let target = relative_target::<T>(&fallthrough, displacement);
    Ok(match condition {
        Some(condition) => execution.condition(condition)?.select(target, fallthrough),
        None => target,
    })
}

fn relative_target<T: RegisterType>(fallthrough: &Val<I32>, displacement: Val<I32>) -> Val<I32>
where
    I32: AtLeast<T>,
{
    fallthrough
        .add(displacement)
        .truncate::<T>()
        .unsigned()
        .extend::<I32>()
}

fn call_relative<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    displacement: Operand<Val<I32>>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let displacement = Input::<I32>::new(displacement).read(execution)?;
    let target = relative_target::<T>(&fallthrough, displacement);
    execution.push(fallthrough.truncate::<T>())?;
    Ok(target)
}

fn call_indirect<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Operand<Val<I32>>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    // The target observes entry registers and memory before the return-address push.
    let target = Input::<T>::new(source).read(execution)?;
    execution.push(fallthrough.truncate::<T>())?;
    Ok(target.unsigned().extend::<I32>())
}

fn jump_indirect<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Operand<Val<I32>>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let target = Input::<T>::new(source).read(execution)?;
    Ok(target.unsigned().extend::<I32>())
}

fn return_near<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    discard_bytes: Operand<Val<I32>>,
    _condition: Option<Condition>,
    _fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError>
where
    I32: AtLeast<T>,
{
    let discard_bytes = Input::<I16>::new(discard_bytes).read(execution)?;
    let target = execution.pop_value::<T>(discard_bytes.unsigned().extend::<I32>())?;
    Ok(target.unsigned().extend::<I32>())
}

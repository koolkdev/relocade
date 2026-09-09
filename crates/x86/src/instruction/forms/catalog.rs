mod binary;
mod moves;
mod unary;

use super::*;

/// Position of an encoded register in the semantic operand pair.
#[derive(Clone, Copy)]
enum RegisterRole {
    Left,
    Right,
}

const fn primary_form(
    opcode: u8,
    width: WidthRule,
    encoding: Encoding,
    operation: Operation,
) -> Form {
    Form {
        opcode,
        mask: 0xff,
        map: OpcodeMap::Primary,
        width,
        encoding,
        extension: None,
        operation,
    }
}

const fn register_rm(
    opcode: u8,
    width: WidthRule,
    register: RegisterRole,
    operation: BinaryOperation,
) -> Form {
    let (left, right) = match register {
        RegisterRole::Left => (LocationBinding::Register, LocationBinding::Rm),
        RegisterRole::Right => (LocationBinding::Rm, LocationBinding::Register),
    };
    primary_form(
        opcode,
        width,
        Encoding::RegisterRm,
        Operation::Binary {
            operation,
            left,
            right: OperandBinding::Location(right),
        },
    )
}

const fn accumulator_immediate(opcode: u8, width: WidthRule, operation: BinaryOperation) -> Form {
    primary_form(
        opcode,
        width,
        Encoding::AccumulatorImmediate,
        Operation::Binary {
            operation,
            left: LocationBinding::Accumulator,
            right: OperandBinding::Immediate,
        },
    )
}

const fn rm_immediate(
    opcode: u8,
    width: WidthRule,
    extension: u8,
    immediate: ImmediateWidth,
    operation: BinaryOperation,
) -> Form {
    let mut form = primary_form(
        opcode,
        width,
        Encoding::RmImmediate { immediate },
        Operation::Binary {
            operation,
            left: LocationBinding::Rm,
            right: OperandBinding::Immediate,
        },
    );
    form.extension = Some(extension);
    form
}

const fn set_condition_forms() -> [Form; 16] {
    let first = Form {
        opcode: 0x90,
        mask: 0xff,
        map: OpcodeMap::Extended,
        encoding: Encoding::Rm,
        extension: None,
        width: WidthRule::Byte,
        operation: Operation::SetCondition(Condition::from_code(0)),
    };
    let mut forms = [first; 16];
    let mut code = 0;
    while code < forms.len() {
        forms[code].opcode += code as u8;
        forms[code].operation = Operation::SetCondition(Condition::from_code(code as u8));
        code += 1;
    }
    forms
}

const SET_CONDITION_FORMS: [Form; 16] = set_condition_forms();

fn primary_forms() -> impl Iterator<Item = &'static Form> + Clone {
    moves::forms()
        .chain(binary::modrm_forms())
        .chain(binary::accumulator_immediate_forms())
        .chain(unary::FORMS.iter())
}

pub(crate) fn modrm_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    opcode_forms(map).filter(|form| form.encoding.has_modrm())
}

pub(crate) fn opcode_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    primary_forms()
        .chain(SET_CONDITION_FORMS.iter())
        .filter(move |form| form.map == map)
}

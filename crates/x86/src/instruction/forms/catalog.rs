use super::*;

pub(crate) const MOV_OPERAND_IMMEDIATE: Form = Form {
    opcode: 0xb8,
    map: OpcodeMap::Primary,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: WidthRule::OperandSize,
    operation: Operation::Binary(BinaryOperation::Mov),
};

pub(crate) const MOV_BYTE_IMMEDIATE: Form = Form {
    opcode: 0xb0,
    map: OpcodeMap::Primary,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: WidthRule::Byte,
    operation: Operation::Binary(BinaryOperation::Mov),
};

pub(crate) const OPCODE_REGISTER_IMMEDIATE_FORMS: [Form; 2] =
    [MOV_OPERAND_IMMEDIATE, MOV_BYTE_IMMEDIATE];

pub(crate) const MOV_MODRM_FORMS: [Form; 6] = [
    register_rm(
        0x89,
        WidthRule::OperandSize,
        RegisterRole::Right,
        BinaryOperation::Mov,
    ),
    register_rm(
        0x8b,
        WidthRule::OperandSize,
        RegisterRole::Left,
        BinaryOperation::Mov,
    ),
    register_rm(
        0x88,
        WidthRule::Byte,
        RegisterRole::Right,
        BinaryOperation::Mov,
    ),
    register_rm(
        0x8a,
        WidthRule::Byte,
        RegisterRole::Left,
        BinaryOperation::Mov,
    ),
    rm_immediate(
        0xc6,
        WidthRule::Byte,
        0,
        ImmediateWidth::Operand,
        BinaryOperation::Mov,
    ),
    rm_immediate(
        0xc7,
        WidthRule::OperandSize,
        0,
        ImmediateWidth::Operand,
        BinaryOperation::Mov,
    ),
];

pub(crate) const ACCUMULATOR_OFFSET_FORMS: [Form; 4] = [
    Form {
        opcode: 0xa0,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Left,
        },
        width: WidthRule::Byte,
        operation: Operation::Binary(BinaryOperation::Mov),
    },
    Form {
        opcode: 0xa1,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Left,
        },
        width: WidthRule::OperandSize,
        operation: Operation::Binary(BinaryOperation::Mov),
    },
    Form {
        opcode: 0xa2,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Right,
        },
        width: WidthRule::Byte,
        operation: Operation::Binary(BinaryOperation::Mov),
    },
    Form {
        opcode: 0xa3,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Right,
        },
        width: WidthRule::OperandSize,
        operation: Operation::Binary(BinaryOperation::Mov),
    },
];

const fn binary_form(
    opcode: u8,
    width: WidthRule,
    encoding: Encoding,
    operation: BinaryOperation,
) -> Form {
    Form {
        opcode,
        mask: 0xff,
        map: OpcodeMap::Primary,
        width,
        encoding,
        operation: Operation::Binary(operation),
    }
}
const fn register_rm(
    opcode: u8,
    width: WidthRule,
    register: RegisterRole,
    operation: BinaryOperation,
) -> Form {
    binary_form(opcode, width, Encoding::RegisterRm { register }, operation)
}
const fn rm_immediate(
    opcode: u8,
    width: WidthRule,
    extension: u8,
    immediate: ImmediateWidth,
    operation: BinaryOperation,
) -> Form {
    binary_form(
        opcode,
        width,
        Encoding::RmImmediate {
            extension,
            immediate,
        },
        operation,
    )
}

pub(crate) const ARITHMETIC_MODRM_FORMS: [Form; 14] = [
    register_rm(
        0x00,
        WidthRule::Byte,
        RegisterRole::Right,
        BinaryOperation::Add,
    ),
    register_rm(
        0x01,
        WidthRule::OperandSize,
        RegisterRole::Right,
        BinaryOperation::Add,
    ),
    register_rm(
        0x02,
        WidthRule::Byte,
        RegisterRole::Left,
        BinaryOperation::Add,
    ),
    register_rm(
        0x03,
        WidthRule::OperandSize,
        RegisterRole::Left,
        BinaryOperation::Add,
    ),
    register_rm(
        0x38,
        WidthRule::Byte,
        RegisterRole::Right,
        BinaryOperation::Compare,
    ),
    register_rm(
        0x39,
        WidthRule::OperandSize,
        RegisterRole::Right,
        BinaryOperation::Compare,
    ),
    register_rm(
        0x3a,
        WidthRule::Byte,
        RegisterRole::Left,
        BinaryOperation::Compare,
    ),
    register_rm(
        0x3b,
        WidthRule::OperandSize,
        RegisterRole::Left,
        BinaryOperation::Compare,
    ),
    rm_immediate(
        0x80,
        WidthRule::Byte,
        0,
        ImmediateWidth::Operand,
        BinaryOperation::Add,
    ),
    rm_immediate(
        0x80,
        WidthRule::Byte,
        7,
        ImmediateWidth::Operand,
        BinaryOperation::Compare,
    ),
    rm_immediate(
        0x81,
        WidthRule::OperandSize,
        0,
        ImmediateWidth::Operand,
        BinaryOperation::Add,
    ),
    rm_immediate(
        0x81,
        WidthRule::OperandSize,
        7,
        ImmediateWidth::Operand,
        BinaryOperation::Compare,
    ),
    rm_immediate(
        0x83,
        WidthRule::OperandSize,
        0,
        ImmediateWidth::SignedByte,
        BinaryOperation::Add,
    ),
    rm_immediate(
        0x83,
        WidthRule::OperandSize,
        7,
        ImmediateWidth::SignedByte,
        BinaryOperation::Compare,
    ),
];

pub(crate) const ACCUMULATOR_IMMEDIATE_FORMS: [Form; 4] = [
    binary_form(
        0x04,
        WidthRule::Byte,
        Encoding::AccumulatorImmediate,
        BinaryOperation::Add,
    ),
    binary_form(
        0x05,
        WidthRule::OperandSize,
        Encoding::AccumulatorImmediate,
        BinaryOperation::Add,
    ),
    binary_form(
        0x3c,
        WidthRule::Byte,
        Encoding::AccumulatorImmediate,
        BinaryOperation::Compare,
    ),
    binary_form(
        0x3d,
        WidthRule::OperandSize,
        Encoding::AccumulatorImmediate,
        BinaryOperation::Compare,
    ),
];

const fn set_condition_forms() -> [Form; 16] {
    let first = Form {
        opcode: 0x90,
        mask: 0xff,
        map: OpcodeMap::Extended,
        encoding: Encoding::Rm,
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

pub(crate) const SET_CONDITION_FORMS: [Form; 16] = set_condition_forms();

pub(crate) fn primary_forms() -> impl Iterator<Item = &'static Form> + Clone {
    OPCODE_REGISTER_IMMEDIATE_FORMS
        .iter()
        .chain(MOV_MODRM_FORMS.iter())
        .chain(ACCUMULATOR_OFFSET_FORMS.iter())
        .chain(ARITHMETIC_MODRM_FORMS.iter())
        .chain(ACCUMULATOR_IMMEDIATE_FORMS.iter())
}

pub(crate) fn modrm_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    MOV_MODRM_FORMS
        .iter()
        .chain(ARITHMETIC_MODRM_FORMS.iter())
        .chain(SET_CONDITION_FORMS.iter())
        .filter(move |form| form.map == map)
}

/// SETcc occupies one complete sixteen-selector family in the extended map.
pub(crate) fn is_set_condition(selector: &Val<I8>) -> Val<I1> {
    selector.and(0xf0).eq(0x90)
}

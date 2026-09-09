use super::*;

const MOV_OPERAND_IMMEDIATE: Form = Form {
    opcode: 0xb8,
    map: OpcodeMap::Primary,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    extension: None,
    width: WidthRule::OperandSize,
    operation: Operation::Binary {
        operation: BinaryOperation::Mov,
        left: LocationBinding::Register,
        right: OperandBinding::Immediate,
    },
};

const MOV_BYTE_IMMEDIATE: Form = Form {
    opcode: 0xb0,
    map: OpcodeMap::Primary,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    extension: None,
    width: WidthRule::Byte,
    operation: Operation::Binary {
        operation: BinaryOperation::Mov,
        left: LocationBinding::Register,
        right: OperandBinding::Immediate,
    },
};

const OPCODE_REGISTER_IMMEDIATE_FORMS: [Form; 2] = [MOV_OPERAND_IMMEDIATE, MOV_BYTE_IMMEDIATE];

const MOV_MODRM_FORMS: [Form; 6] = [
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

const ACCUMULATOR_OFFSET_FORMS: [Form; 4] = [
    Form {
        opcode: 0xa0,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset,
        extension: None,
        width: WidthRule::Byte,
        operation: Operation::Binary {
            operation: BinaryOperation::Mov,
            left: LocationBinding::Accumulator,
            right: OperandBinding::Location(LocationBinding::AbsoluteOffset),
        },
    },
    Form {
        opcode: 0xa1,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset,
        extension: None,
        width: WidthRule::OperandSize,
        operation: Operation::Binary {
            operation: BinaryOperation::Mov,
            left: LocationBinding::Accumulator,
            right: OperandBinding::Location(LocationBinding::AbsoluteOffset),
        },
    },
    Form {
        opcode: 0xa2,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset,
        extension: None,
        width: WidthRule::Byte,
        operation: Operation::Binary {
            operation: BinaryOperation::Mov,
            left: LocationBinding::AbsoluteOffset,
            right: OperandBinding::Location(LocationBinding::Accumulator),
        },
    },
    Form {
        opcode: 0xa3,
        map: OpcodeMap::Primary,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset,
        extension: None,
        width: WidthRule::OperandSize,
        operation: Operation::Binary {
            operation: BinaryOperation::Mov,
            left: LocationBinding::AbsoluteOffset,
            right: OperandBinding::Location(LocationBinding::Accumulator),
        },
    },
];

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    OPCODE_REGISTER_IMMEDIATE_FORMS
        .iter()
        .chain(MOV_MODRM_FORMS.iter())
        .chain(ACCUMULATOR_OFFSET_FORMS.iter())
}

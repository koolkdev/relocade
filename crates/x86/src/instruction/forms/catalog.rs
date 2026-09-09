use super::*;

const MOV_OPERAND_IMMEDIATE: Form = Form {
    opcode: 0xb8,
    map: OpcodeMap::Primary,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: WidthRule::OperandSize,
    operation: Operation::Binary(BinaryOperation::Mov),
};

const MOV_BYTE_IMMEDIATE: Form = Form {
    opcode: 0xb0,
    map: OpcodeMap::Primary,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: WidthRule::Byte,
    operation: Operation::Binary(BinaryOperation::Mov),
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

/// The ordinary binary families share operand layouts. Bits 3–5 of the first
/// opcode also select their extension in groups 80, 81 and 83.
struct BinaryFamily {
    register_rm: [Form; 4],
    accumulator_immediate: [Form; 2],
    rm_immediate: [Form; 3],
}

const fn binary_family(first_opcode: u8, operation: BinaryOperation) -> BinaryFamily {
    let extension = first_opcode >> 3;
    BinaryFamily {
        register_rm: [
            register_rm(
                first_opcode,
                WidthRule::Byte,
                RegisterRole::Right,
                operation,
            ),
            register_rm(
                first_opcode + 1,
                WidthRule::OperandSize,
                RegisterRole::Right,
                operation,
            ),
            register_rm(
                first_opcode + 2,
                WidthRule::Byte,
                RegisterRole::Left,
                operation,
            ),
            register_rm(
                first_opcode + 3,
                WidthRule::OperandSize,
                RegisterRole::Left,
                operation,
            ),
        ],
        accumulator_immediate: [
            binary_form(
                first_opcode + 4,
                WidthRule::Byte,
                Encoding::AccumulatorImmediate,
                operation,
            ),
            binary_form(
                first_opcode + 5,
                WidthRule::OperandSize,
                Encoding::AccumulatorImmediate,
                operation,
            ),
        ],
        rm_immediate: [
            rm_immediate(
                0x80,
                WidthRule::Byte,
                extension,
                ImmediateWidth::Operand,
                operation,
            ),
            rm_immediate(
                0x81,
                WidthRule::OperandSize,
                extension,
                ImmediateWidth::Operand,
                operation,
            ),
            rm_immediate(
                0x83,
                WidthRule::OperandSize,
                extension,
                ImmediateWidth::SignedByte,
                operation,
            ),
        ],
    }
}

const BINARY_FAMILIES: [BinaryFamily; 6] = [
    binary_family(0x00, BinaryOperation::Add),
    binary_family(0x38, BinaryOperation::Compare),
    binary_family(0x28, BinaryOperation::Subtract),
    binary_family(0x20, BinaryOperation::And),
    binary_family(0x08, BinaryOperation::Or),
    binary_family(0x30, BinaryOperation::Xor),
];

const TEST_MODRM_FORMS: [Form; 4] = [
    register_rm(
        0x84,
        WidthRule::Byte,
        RegisterRole::Right,
        BinaryOperation::Test,
    ),
    register_rm(
        0x85,
        WidthRule::OperandSize,
        RegisterRole::Right,
        BinaryOperation::Test,
    ),
    rm_immediate(
        0xf6,
        WidthRule::Byte,
        0,
        ImmediateWidth::Operand,
        BinaryOperation::Test,
    ),
    rm_immediate(
        0xf7,
        WidthRule::OperandSize,
        0,
        ImmediateWidth::Operand,
        BinaryOperation::Test,
    ),
];

const TEST_ACCUMULATOR_FORMS: [Form; 2] = [
    binary_form(
        0xa8,
        WidthRule::Byte,
        Encoding::AccumulatorImmediate,
        BinaryOperation::Test,
    ),
    binary_form(
        0xa9,
        WidthRule::OperandSize,
        Encoding::AccumulatorImmediate,
        BinaryOperation::Test,
    ),
];

fn binary_modrm_forms() -> impl Iterator<Item = &'static Form> + Clone {
    BINARY_FAMILIES
        .iter()
        .flat_map(|family| family.register_rm.iter())
        .chain(
            BINARY_FAMILIES
                .iter()
                .flat_map(|family| family.rm_immediate.iter()),
        )
        .chain(TEST_MODRM_FORMS.iter())
}

fn accumulator_immediate_forms() -> impl Iterator<Item = &'static Form> + Clone {
    BINARY_FAMILIES
        .iter()
        .flat_map(|family| family.accumulator_immediate.iter())
        .chain(TEST_ACCUMULATOR_FORMS.iter())
}

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

const SET_CONDITION_FORMS: [Form; 16] = set_condition_forms();

fn primary_forms() -> impl Iterator<Item = &'static Form> + Clone {
    OPCODE_REGISTER_IMMEDIATE_FORMS
        .iter()
        .chain(MOV_MODRM_FORMS.iter())
        .chain(ACCUMULATOR_OFFSET_FORMS.iter())
        .chain(binary_modrm_forms())
        .chain(accumulator_immediate_forms())
}

pub(crate) fn modrm_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    opcode_forms(map).filter(|form| form.encoding.has_modrm())
}

pub(crate) fn opcode_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    primary_forms()
        .chain(SET_CONDITION_FORMS.iter())
        .filter(move |form| form.map == map)
}

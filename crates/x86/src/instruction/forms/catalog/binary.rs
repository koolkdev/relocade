use super::*;

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
            accumulator_immediate(first_opcode + 4, WidthRule::Byte, operation),
            accumulator_immediate(first_opcode + 5, WidthRule::OperandSize, operation),
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

const BINARY_FAMILIES: [BinaryFamily; 8] = [
    binary_family(0x00, BinaryOperation::Add),
    binary_family(0x10, BinaryOperation::AddWithCarry),
    binary_family(0x18, BinaryOperation::SubtractWithBorrow),
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
    accumulator_immediate(0xa8, WidthRule::Byte, BinaryOperation::Test),
    accumulator_immediate(0xa9, WidthRule::OperandSize, BinaryOperation::Test),
];

pub(super) fn modrm_forms() -> impl Iterator<Item = &'static Form> + Clone {
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

pub(super) fn accumulator_immediate_forms() -> impl Iterator<Item = &'static Form> + Clone {
    BINARY_FAMILIES
        .iter()
        .flat_map(|family| family.accumulator_immediate.iter())
        .chain(TEST_ACCUMULATOR_FORMS.iter())
}

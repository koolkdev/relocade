use super::*;

const fn opcode_register(opcode: u8, operation: UnaryOperation) -> Form {
    let mut form = primary_form(
        opcode,
        WidthRule::OperandSize,
        Encoding::OpcodeRegister,
        Operation::Unary(operation),
    );
    form.mask = 0xf8;
    form
}

const fn rm(opcode: u8, width: WidthRule, extension: u8, operation: UnaryOperation) -> Form {
    let mut form = primary_form(opcode, width, Encoding::Rm, Operation::Unary(operation));
    form.extension = Some(extension);
    form
}

pub(super) const FORMS: [Form; 10] = [
    opcode_register(0x40, UnaryOperation::Increment),
    opcode_register(0x48, UnaryOperation::Decrement),
    rm(0xfe, WidthRule::Byte, 0, UnaryOperation::Increment),
    rm(0xff, WidthRule::OperandSize, 0, UnaryOperation::Increment),
    rm(0xfe, WidthRule::Byte, 1, UnaryOperation::Decrement),
    rm(0xff, WidthRule::OperandSize, 1, UnaryOperation::Decrement),
    rm(0xf6, WidthRule::Byte, 2, UnaryOperation::Not),
    rm(0xf7, WidthRule::OperandSize, 2, UnaryOperation::Not),
    rm(0xf6, WidthRule::Byte, 3, UnaryOperation::Negate),
    rm(0xf7, WidthRule::OperandSize, 3, UnaryOperation::Negate),
];

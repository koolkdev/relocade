use super::*;

const fn push_immediate(opcode: u8, immediate: ImmediateWidth) -> Form {
    primary_form(
        opcode,
        WidthRule::OperandSize,
        Encoding::Immediate { immediate },
        Operation::Push(OperandBinding::Immediate),
    )
}

pub(super) const FORMS: [Form; 6] = [
    opcode_register(
        0x50,
        Operation::Push(OperandBinding::Location(LocationBinding::Register)),
    ),
    opcode_register(0x58, Operation::Pop(LocationBinding::Register)),
    push_immediate(0x68, ImmediateWidth::Operand),
    push_immediate(0x6a, ImmediateWidth::SignedByte),
    rm(
        0xff,
        WidthRule::OperandSize,
        6,
        Operation::Push(OperandBinding::Location(LocationBinding::Rm)),
    ),
    rm(
        0x8f,
        WidthRule::OperandSize,
        0,
        Operation::Pop(LocationBinding::Rm),
    ),
];

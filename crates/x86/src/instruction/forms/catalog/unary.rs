use super::*;
use Operation::Unary;
use UnaryOperation::{Decrement, Increment, Negate, Not};

pub(super) const FORMS: [Form; 10] = [
    opcode_register(0x40, Unary(Increment)),
    opcode_register(0x48, Unary(Decrement)),
    rm(0xfe, WidthRule::Byte, 0, Unary(Increment)),
    rm(0xff, WidthRule::OperandSize, 0, Unary(Increment)),
    rm(0xfe, WidthRule::Byte, 1, Unary(Decrement)),
    rm(0xff, WidthRule::OperandSize, 1, Unary(Decrement)),
    rm(0xf6, WidthRule::Byte, 2, Unary(Not)),
    rm(0xf7, WidthRule::OperandSize, 2, Unary(Not)),
    rm(0xf6, WidthRule::Byte, 3, Unary(Negate)),
    rm(0xf7, WidthRule::OperandSize, 3, Unary(Negate)),
];

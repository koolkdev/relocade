//! Binary arithmetic combines an x87 destination with a register or real memory source.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::X87Operand,
    instruction::X87StackIndex,
    x87::{BinaryFormat, BinaryOperation},
};

instruction_families! {
    FADD_BINARY32 {
        execute: binary_memory(BinaryOperation::Add, BinaryFormat::Binary32);
        forms { 0xD8 / 0 => operands(mem); }
    }
    FADD_BINARY64 {
        execute: binary_memory(BinaryOperation::Add, BinaryFormat::Binary64);
        forms { 0xDC / 0 => operands(mem); }
    }
    FSUB_BINARY32 {
        execute: binary_memory(BinaryOperation::Subtract, BinaryFormat::Binary32);
        forms { 0xD8 / 4 => operands(mem); }
    }
    FSUB_BINARY64 {
        execute: binary_memory(BinaryOperation::Subtract, BinaryFormat::Binary64);
        forms { 0xDC / 4 => operands(mem); }
    }
    FSUBR_BINARY32 {
        execute: binary_memory(BinaryOperation::ReverseSubtract, BinaryFormat::Binary32);
        forms { 0xD8 / 5 => operands(mem); }
    }
    FSUBR_BINARY64 {
        execute: binary_memory(BinaryOperation::ReverseSubtract, BinaryFormat::Binary64);
        forms { 0xDC / 5 => operands(mem); }
    }
    FMUL_BINARY32 {
        execute: binary_memory(BinaryOperation::Multiply, BinaryFormat::Binary32);
        forms { 0xD8 / 1 => operands(mem); }
    }
    FMUL_BINARY64 {
        execute: binary_memory(BinaryOperation::Multiply, BinaryFormat::Binary64);
        forms { 0xDC / 1 => operands(mem); }
    }
    FDIV_BINARY32 {
        execute: binary_memory(BinaryOperation::Divide, BinaryFormat::Binary32);
        forms { 0xD8 / 6 => operands(mem); }
    }
    FDIV_BINARY64 {
        execute: binary_memory(BinaryOperation::Divide, BinaryFormat::Binary64);
        forms { 0xDC / 6 => operands(mem); }
    }
    FDIVR_BINARY32 {
        execute: binary_memory(BinaryOperation::ReverseDivide, BinaryFormat::Binary32);
        forms { 0xD8 / 7 => operands(mem); }
    }
    FDIVR_BINARY64 {
        execute: binary_memory(BinaryOperation::ReverseDivide, BinaryFormat::Binary64);
        forms { 0xDC / 7 => operands(mem); }
    }
    FADD_TOP {
        execute: binary_register(BinaryOperation::Add, Destination::Top, false);
        forms { 0xD8 @ 0xC0 + rm => operands(st); }
    }
    FADD_REGISTER {
        execute: binary_register(BinaryOperation::Add, Destination::Other, false);
        forms { 0xDC @ 0xC0 + rm => operands(st); }
    }
    FADDP_REGISTER {
        execute: binary_register(BinaryOperation::Add, Destination::Other, true);
        forms { 0xDE @ 0xC0 + rm => operands(st); }
    }
    FSUB_TOP {
        execute: binary_register(BinaryOperation::Subtract, Destination::Top, false);
        forms { 0xD8 @ 0xE0 + rm => operands(st); }
    }
    FSUB_REGISTER {
        execute: binary_register(BinaryOperation::Subtract, Destination::Other, false);
        forms { 0xDC @ 0xE8 + rm => operands(st); }
    }
    FSUBP_REGISTER {
        execute: binary_register(BinaryOperation::Subtract, Destination::Other, true);
        forms { 0xDE @ 0xE8 + rm => operands(st); }
    }
    FSUBR_TOP {
        execute: binary_register(BinaryOperation::ReverseSubtract, Destination::Top, false);
        forms { 0xD8 @ 0xE8 + rm => operands(st); }
    }
    FSUBR_REGISTER {
        execute: binary_register(BinaryOperation::ReverseSubtract, Destination::Other, false);
        forms { 0xDC @ 0xE0 + rm => operands(st); }
    }
    FSUBRP_REGISTER {
        execute: binary_register(BinaryOperation::ReverseSubtract, Destination::Other, true);
        forms { 0xDE @ 0xE0 + rm => operands(st); }
    }
    FMUL_TOP {
        execute: binary_register(BinaryOperation::Multiply, Destination::Top, false);
        forms { 0xD8 @ 0xC8 + rm => operands(st); }
    }
    FMUL_REGISTER {
        execute: binary_register(BinaryOperation::Multiply, Destination::Other, false);
        forms { 0xDC @ 0xC8 + rm => operands(st); }
    }
    FMULP_REGISTER {
        execute: binary_register(BinaryOperation::Multiply, Destination::Other, true);
        forms { 0xDE @ 0xC8 + rm => operands(st); }
    }
    FDIV_TOP {
        execute: binary_register(BinaryOperation::Divide, Destination::Top, false);
        forms { 0xD8 @ 0xF0 + rm => operands(st); }
    }
    FDIV_REGISTER {
        execute: binary_register(BinaryOperation::Divide, Destination::Other, false);
        forms { 0xDC @ 0xF8 + rm => operands(st); }
    }
    FDIVP_REGISTER {
        execute: binary_register(BinaryOperation::Divide, Destination::Other, true);
        forms { 0xDE @ 0xF8 + rm => operands(st); }
    }
    FDIVR_TOP {
        execute: binary_register(BinaryOperation::ReverseDivide, Destination::Top, false);
        forms { 0xD8 @ 0xF8 + rm => operands(st); }
    }
    FDIVR_REGISTER {
        execute: binary_register(BinaryOperation::ReverseDivide, Destination::Other, false);
        forms { 0xDC @ 0xF0 + rm => operands(st); }
    }
    FDIVRP_REGISTER {
        execute: binary_register(BinaryOperation::ReverseDivide, Destination::Other, true);
        forms { 0xDE @ 0xF0 + rm => operands(st); }
    }
}

enum Destination {
    Top,
    Other,
}

fn binary_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    other: X87StackIndex,
    operation: BinaryOperation,
    destination: Destination,
    pop: bool,
) -> Result<(), BuildError> {
    let (destination, source) = match destination {
        Destination::Top => (0.into(), other.offset()),
        Destination::Other => (other.offset(), 0.into()),
    };
    execution.arithmetic_x87(destination, X87Operand::Register(source), operation, pop)
}

fn binary_memory(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    operation: BinaryOperation,
    format: BinaryFormat,
) -> Result<(), BuildError> {
    execution.arithmetic_x87(
        0.into(),
        X87Operand::BinaryMemory { address, format },
        operation,
        false,
    )
}

use super::*;
use crate::alu::{AnyStatusSource, ArithmeticOp, LogicOp, StatusSource, UnaryOp};
use crate::register::RegisterType;

#[derive(Clone, Copy)]
enum BinaryOperation {
    Add,
    AddWithCarry,
    Subtract,
    SubtractWithBorrow,
    And,
    Or,
    Xor,
}

instruction_families! {
    ADD {
        execute: update_binary(BinaryOperation::Add);
        forms {
            0x00 => byte(rm, modrm_reg);
            0x01 => word_or_dword(rm, modrm_reg);
            0x02 => byte(modrm_reg, rm);
            0x03 => word_or_dword(modrm_reg, rm);
            0x04 => byte(accumulator, imm8);
            0x05 => word_or_dword(accumulator, imm);
            0x80 /0 => byte(rm, imm8);
            0x81 /0 => word_or_dword(rm, imm);
            0x83 /0 => word_or_dword(rm, signed_imm8);
        }
    }
    ADC {
        execute: update_binary(BinaryOperation::AddWithCarry);
        forms {
            0x10 => byte(rm, modrm_reg);
            0x11 => word_or_dword(rm, modrm_reg);
            0x12 => byte(modrm_reg, rm);
            0x13 => word_or_dword(modrm_reg, rm);
            0x14 => byte(accumulator, imm8);
            0x15 => word_or_dword(accumulator, imm);
            0x80 /2 => byte(rm, imm8);
            0x81 /2 => word_or_dword(rm, imm);
            0x83 /2 => word_or_dword(rm, signed_imm8);
        }
    }
    SBB {
        execute: update_binary(BinaryOperation::SubtractWithBorrow);
        forms {
            0x18 => byte(rm, modrm_reg);
            0x19 => word_or_dword(rm, modrm_reg);
            0x1A => byte(modrm_reg, rm);
            0x1B => word_or_dword(modrm_reg, rm);
            0x1C => byte(accumulator, imm8);
            0x1D => word_or_dword(accumulator, imm);
            0x80 /3 => byte(rm, imm8);
            0x81 /3 => word_or_dword(rm, imm);
            0x83 /3 => word_or_dword(rm, signed_imm8);
        }
    }
    CMP {
        execute: compare;
        forms {
            0x38 => byte(rm, modrm_reg);
            0x39 => word_or_dword(rm, modrm_reg);
            0x3A => byte(modrm_reg, rm);
            0x3B => word_or_dword(modrm_reg, rm);
            0x3C => byte(accumulator, imm8);
            0x3D => word_or_dword(accumulator, imm);
            0x80 /7 => byte(rm, imm8);
            0x81 /7 => word_or_dword(rm, imm);
            0x83 /7 => word_or_dword(rm, signed_imm8);
        }
    }
    SUB {
        execute: update_binary(BinaryOperation::Subtract);
        forms {
            0x28 => byte(rm, modrm_reg);
            0x29 => word_or_dword(rm, modrm_reg);
            0x2A => byte(modrm_reg, rm);
            0x2B => word_or_dword(modrm_reg, rm);
            0x2C => byte(accumulator, imm8);
            0x2D => word_or_dword(accumulator, imm);
            0x80 /5 => byte(rm, imm8);
            0x81 /5 => word_or_dword(rm, imm);
            0x83 /5 => word_or_dword(rm, signed_imm8);
        }
    }
    AND {
        execute: update_binary(BinaryOperation::And);
        forms {
            0x20 => byte(rm, modrm_reg);
            0x21 => word_or_dword(rm, modrm_reg);
            0x22 => byte(modrm_reg, rm);
            0x23 => word_or_dword(modrm_reg, rm);
            0x24 => byte(accumulator, imm8);
            0x25 => word_or_dword(accumulator, imm);
            0x80 /4 => byte(rm, imm8);
            0x81 /4 => word_or_dword(rm, imm);
            0x83 /4 => word_or_dword(rm, signed_imm8);
        }
    }
    OR {
        execute: update_binary(BinaryOperation::Or);
        forms {
            0x08 => byte(rm, modrm_reg);
            0x09 => word_or_dword(rm, modrm_reg);
            0x0A => byte(modrm_reg, rm);
            0x0B => word_or_dword(modrm_reg, rm);
            0x0C => byte(accumulator, imm8);
            0x0D => word_or_dword(accumulator, imm);
            0x80 /1 => byte(rm, imm8);
            0x81 /1 => word_or_dword(rm, imm);
            0x83 /1 => word_or_dword(rm, signed_imm8);
        }
    }
    XOR {
        execute: update_binary(BinaryOperation::Xor);
        forms {
            0x30 => byte(rm, modrm_reg);
            0x31 => word_or_dword(rm, modrm_reg);
            0x32 => byte(modrm_reg, rm);
            0x33 => word_or_dword(modrm_reg, rm);
            0x34 => byte(accumulator, imm8);
            0x35 => word_or_dword(accumulator, imm);
            0x80 /6 => byte(rm, imm8);
            0x81 /6 => word_or_dword(rm, imm);
            0x83 /6 => word_or_dword(rm, signed_imm8);
        }
    }
    TEST {
        execute: test;
        forms {
            0x84 => byte(rm, modrm_reg);
            0x85 => word_or_dword(rm, modrm_reg);
            0xF6 /0 => byte(rm, imm8);
            0xF7 /0 => word_or_dword(rm, imm);
            0xA8 => byte(accumulator, imm8);
            0xA9 => word_or_dword(accumulator, imm);
        }
    }
    INC {
        execute: update_unary(UnaryOp::Increment);
        forms {
            0x40 +reg => word_or_dword(opcode_reg);
            0xFE /0 => byte(rm);
            0xFF /0 => word_or_dword(rm);
        }
    }
    DEC {
        execute: update_unary(UnaryOp::Decrement);
        forms {
            0x48 +reg => word_or_dword(opcode_reg);
            0xFE /1 => byte(rm);
            0xFF /1 => word_or_dword(rm);
        }
    }
    NOT {
        execute: update_unary(UnaryOp::Not);
        forms {
            0xF6 /2 => byte(rm);
            0xF7 /2 => word_or_dword(rm);
        }
    }
    NEG {
        execute: update_unary(UnaryOp::Negate);
        forms {
            0xF6 /3 => byte(rm);
            0xF7 /3 => word_or_dword(rm);
        }
    }
}

fn update_binary<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
    operation: BinaryOperation,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    destination.update(execution, |execution, left| {
        let right = source.read(execution)?;
        let outcome = match operation {
            BinaryOperation::Add => ArithmeticOp::Add.apply(left, right),
            BinaryOperation::Subtract => ArithmeticOp::Subtract.apply(left, right),
            BinaryOperation::AddWithCarry => {
                let carry = execution.read_flag(crate::flags::Flag::CF)?;
                ArithmeticOp::Add.apply_with_carry(left, right, carry)
            }
            BinaryOperation::SubtractWithBorrow => {
                let carry = execution.read_flag(crate::flags::Flag::CF)?;
                ArithmeticOp::Subtract.apply_with_carry(left, right, carry)
            }
            BinaryOperation::And => LogicOp::And.apply(left, right),
            BinaryOperation::Or => LogicOp::Or.apply(left, right),
            BinaryOperation::Xor => LogicOp::Xor.apply(left, right),
        };
        execution.write_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

fn compare<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: TypedLocation<T>,
    right: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let left = left.read(execution)?;
    let right = right.read(execution)?;
    execution.write_flags(ArithmeticOp::Subtract.apply(left, right).flags)
}

fn test<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: TypedLocation<T>,
    right: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let left = left.read(execution)?;
    let right = right.read(execution)?;
    execution.write_flags(LogicOp::And.apply(left, right).flags)
}

fn update_unary<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    operation: UnaryOp,
) -> Result<(), BuildError>
where
    StatusSource<T>: Into<AnyStatusSource>,
{
    destination.update(execution, |execution, input| {
        let outcome = operation.apply(input);
        execution.write_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

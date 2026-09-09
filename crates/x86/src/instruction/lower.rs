use wasm86_compiler::{AtLeast, BuildError, MemoryInt, Val, I16, I32, I8};

use super::{
    BinaryInstruction, BinaryOperation, Instruction, OperandWidth, UnaryInstruction, UnaryOperation,
};
use crate::{
    execution::ExecutionBuilder,
    flags::{ArithmeticKind, Condition, FlagSource, LocalFlagSource, StatusFlag},
    register::RegisterType,
};

pub(crate) fn lower(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: Instruction<impl Into<Val<I32>>>,
) -> Result<(), BuildError> {
    let width = match &instruction {
        Instruction::Binary(instruction) => instruction.width,
        Instruction::Unary(instruction) => instruction.width,
        Instruction::SetCondition { .. } => OperandWidth::Byte,
    };
    match width {
        OperandWidth::Byte => lower_typed::<I8>(execution, instruction),
        OperandWidth::Word => lower_typed::<I16>(execution, instruction),
        OperandWidth::Dword => lower_typed::<I32>(execution, instruction),
    }
}

fn lower_typed<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: Instruction<impl Into<Val<I32>>>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<LocalFlagSource>,
{
    match instruction {
        Instruction::Binary(instruction) => lower_binary::<T>(execution, instruction),
        Instruction::Unary(instruction) => lower_unary::<T>(execution, instruction),
        Instruction::SetCondition {
            condition,
            destination,
        } => {
            let value = execution.condition(condition)?;
            execution.write::<I8>(destination, value.unsigned().extend::<I8>())
        }
    }
}

fn lower_binary<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: BinaryInstruction<impl Into<Val<I32>>>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<LocalFlagSource>,
{
    let operation = instruction.operation;
    if matches!(operation, BinaryOperation::Mov) {
        let value = execution.read::<T>(instruction.right)?;
        return execution.write::<T>(instruction.left, value);
    }

    let calculate_result = |execution: &mut ExecutionBuilder<'_, '_>, left: Val<T>| {
        let right = execution.read::<T>(instruction.right)?;
        let source = binary_flags(execution, operation, left, right)?;
        let result = source.result().clone();
        execution.set_flags(source)?;
        Ok(result)
    };

    if matches!(operation, BinaryOperation::Compare | BinaryOperation::Test) {
        let left = execution.read::<T>(instruction.left.into())?;
        calculate_result(execution, left).map(|_| ())
    } else {
        execution.update::<T>(instruction.left, calculate_result)
    }
}

fn binary_flags<T: MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    operation: BinaryOperation,
    left: Val<T>,
    right: Val<T>,
) -> Result<FlagSource<T>, BuildError> {
    Ok(match operation {
        BinaryOperation::Add => FlagSource::arithmetic(ArithmeticKind::Add, left, right),
        BinaryOperation::Subtract | BinaryOperation::Compare => {
            FlagSource::arithmetic(ArithmeticKind::Sub, left, right)
        }
        BinaryOperation::AddWithCarry => {
            let carry = execution.condition(Condition::B)?;
            FlagSource::arithmetic_with_carry(ArithmeticKind::Add, left, right, carry)
        }
        BinaryOperation::SubtractWithBorrow => {
            let carry = execution.condition(Condition::B)?;
            FlagSource::arithmetic_with_carry(ArithmeticKind::Sub, left, right, carry)
        }
        BinaryOperation::And | BinaryOperation::Test => FlagSource::Logic {
            result: left.and(right),
        },
        BinaryOperation::Or => FlagSource::Logic {
            result: left.or(right),
        },
        BinaryOperation::Xor => FlagSource::Logic {
            result: left.xor(right),
        },
        BinaryOperation::Mov => unreachable!("MOV completed without reading its destination"),
    })
}

fn lower_unary<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: UnaryInstruction<impl Into<Val<I32>>>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    FlagSource<T>: Into<LocalFlagSource>,
{
    execution.update::<T>(instruction.destination, |execution, input| {
        let source = match instruction.operation {
            UnaryOperation::Not => return Ok(input.xor(-1)),
            UnaryOperation::Increment | UnaryOperation::Decrement => {
                let carry = execution.condition(Condition::B)?;
                let kind = if matches!(instruction.operation, UnaryOperation::Increment) {
                    ArithmeticKind::Add
                } else {
                    ArithmeticKind::Sub
                };
                FlagSource::arithmetic(kind, input, 1.into()).with_flag(StatusFlag::CF, carry)
            }
            UnaryOperation::Negate => FlagSource::arithmetic(ArithmeticKind::Sub, 0.into(), input),
        };
        let result = source.result().clone();
        execution.set_flags(source)?;
        Ok(result)
    })
}

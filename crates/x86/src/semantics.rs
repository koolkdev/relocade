use wasm86_compiler::{AtLeast, BuildError, IntoOp, Val, I16, I32, I8};

use crate::{
    execution::ExecutionBuilder,
    flags::{ArithmeticSource, FlagSource, LocalFlagSource},
    instruction::{BinaryInstruction, BinaryOperation, Instruction, OperandWidth},
    register::RegisterType,
};

pub(super) fn lower(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: Instruction<impl IntoOp<I32>>,
) -> Result<(), BuildError> {
    match instruction {
        Instruction::Binary(instruction) => match instruction.width {
            OperandWidth::Byte => lower_typed_instruction::<I8>(execution, instruction),
            OperandWidth::Word => lower_typed_instruction::<I16>(execution, instruction),
            OperandWidth::Dword => lower_typed_instruction::<I32>(execution, instruction),
        },
        Instruction::SetCondition {
            condition,
            destination,
        } => {
            let value = execution.condition(condition)?;
            execution.write::<I8>(destination, value.unsigned().extend::<I8>())
        }
    }
}

fn lower_typed_instruction<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: BinaryInstruction<impl IntoOp<I32>>,
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

    let apply_operation = |execution: &mut ExecutionBuilder<'_, '_>, left: Val<T>| {
        let right = execution.read::<T>(instruction.right)?;
        let arithmetic = match operation {
            BinaryOperation::Add => ArithmeticSource::add(left, right),
            BinaryOperation::Subtract | BinaryOperation::Compare => {
                ArithmeticSource::subtract(left, right)
            }
            BinaryOperation::And
            | BinaryOperation::Or
            | BinaryOperation::Xor
            | BinaryOperation::Test => {
                let result = match operation {
                    BinaryOperation::And | BinaryOperation::Test => left.and(right),
                    BinaryOperation::Or => left.or(right),
                    BinaryOperation::Xor => left.xor(right),
                    _ => unreachable!("the handler selected a logical operation"),
                };
                execution.set_logic_flags(&result)?;
                return Ok(result);
            }
            BinaryOperation::Mov => unreachable!("MOV completed without reading its destination"),
        };
        execution.set_arithmetic_flags(&arithmetic)?;
        Ok(arithmetic.result)
    };

    if matches!(operation, BinaryOperation::Compare | BinaryOperation::Test) {
        let left = execution.read::<T>(instruction.left.into())?;
        apply_operation(execution, left).map(|_| ())
    } else {
        execution.update::<T>(instruction.left, apply_operation)
    }
}

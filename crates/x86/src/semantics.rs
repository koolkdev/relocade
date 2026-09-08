use wasm86_compiler::{AtLeast, BuildError, IntoOp, I16, I32, I8};

use crate::{
    execution::ExecutionBuilder,
    flags::{ArithmeticFlagSource, ArithmeticSource},
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
    ArithmeticSource<T>: Into<ArithmeticFlagSource>,
{
    match instruction.operation {
        BinaryOperation::Mov => {
            let value = execution.read::<T>(instruction.right)?;
            execution.write::<T>(instruction.left, value)
        }
        BinaryOperation::Add => execution.update::<T>(instruction.left, |execution, left| {
            let right = execution.read::<T>(instruction.right)?;
            let addition = ArithmeticSource::add(left, right);
            execution.set_arithmetic_flags(&addition)?;
            Ok(addition.result)
        }),
        BinaryOperation::Compare => {
            let left = execution.read::<T>(instruction.left.into())?;
            let right = execution.read::<T>(instruction.right)?;
            execution.set_arithmetic_flags(&ArithmeticSource::subtract(left, right))
        }
    }
}

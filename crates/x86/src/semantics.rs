use wasm86_compiler::{AtLeast, BuildError, IntoOp, I16, I32, I8};

use crate::{
    execution::ExecutionBuilder,
    instruction::{Instruction, OperandWidth, Semantic},
    register::RegisterType,
};

pub(super) fn lower(
    execution: &mut ExecutionBuilder<'_>,
    instruction: Instruction<impl IntoOp<I32>>,
) -> Result<(), BuildError> {
    match instruction.width {
        OperandWidth::Byte => lower_typed_instruction::<I8>(execution, instruction),
        OperandWidth::Word => lower_typed_instruction::<I16>(execution, instruction),
        OperandWidth::Dword => lower_typed_instruction::<I32>(execution, instruction),
    }
}

fn lower_typed_instruction<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_>,
    instruction: Instruction<impl IntoOp<I32>>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    match instruction.semantic {
        Semantic::Mov => {
            let value = execution.read::<T>(instruction.source)?;
            execution.write::<T>(instruction.destination, value)
        }
    }
}

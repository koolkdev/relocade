use wasm86_compiler::{BuildError, IntoOp, I32};

use crate::{
    execution::ExecutionBuilder,
    instruction::{Instruction, Operand32, Semantic},
};

pub(super) fn lower<V: IntoOp<I32>>(
    execution: &mut ExecutionBuilder<'_>,
    instruction: Instruction<V>,
) -> Result<(), BuildError> {
    match instruction.semantic {
        Semantic::Mov32 => match instruction.source {
            Operand32::Immediate(value) => execution.write(instruction.destination, value),
            Operand32::Location(source) => {
                let value = execution.read(source)?;
                execution.write(instruction.destination, value)
            }
        },
    }
}

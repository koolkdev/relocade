use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, I32};

use crate::{
    instruction::{Instruction, Operand32, Semantic},
    state::State,
};

pub(super) fn lower<V: IntoOp<I32>>(
    body: &mut FunctionBuilder<'_>,
    state: &mut State,
    instruction: Instruction<V>,
) -> Result<(), BuildError> {
    match instruction.semantic {
        Semantic::Mov32 => match instruction.source {
            Operand32::Immediate(value) => {
                state.write_register(body, instruction.destination, value)
            }
            Operand32::Register(register) => {
                let source = state.read_register(body, register)?;
                state.write_register(body, instruction.destination, source)
            }
        },
    }
}

use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, I32};

use crate::{
    instruction::{Instruction, Semantic},
    state::State,
};

pub(super) fn lower<V: IntoOp<I32>>(
    body: &mut FunctionBuilder<'_>,
    state: &mut State,
    instruction: Instruction<V>,
) -> Result<(), BuildError> {
    match instruction.semantic {
        Semantic::Mov32 => state.write_register(body, instruction.destination, instruction.source),
    }
}

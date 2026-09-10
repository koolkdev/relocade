use wasm86_compiler::{BuildError, Val, I32};

use super::{handlers::HandlerCall, map_location, map_operand, Instruction};
use crate::execution::ExecutionBuilder;

pub(crate) fn lower(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: Instruction<impl Into<Val<I32>>>,
    fallthrough_eip: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    match instruction.call {
        HandlerCall::Binary {
            handler,
            left,
            right,
        } => handler(
            execution,
            map_location(left),
            map_operand(right),
            instruction.condition,
            fallthrough_eip,
        ),
        HandlerCall::Unary { handler, operand } => handler(
            execution,
            map_operand(operand),
            instruction.condition,
            fallthrough_eip,
        ),
    }
}

use wasm86_compiler::{BuildError, Val, I32};

use super::{handlers::HandlerCall, map_location, map_operand, Instruction};
use crate::execution::{ExecutionBuilder, Successor};

pub(crate) fn lower(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: Instruction<impl Into<Val<I32>>>,
    fallthrough_eip: Val<I32>,
) -> Result<Successor, BuildError> {
    if let Some(exception) = instruction.requirements.fault(execution.target()) {
        execution.fault(exception)?;
        return Ok(fallthrough_eip.into());
    }
    match instruction.call {
        HandlerCall::Nullary { handler } => {
            handler(execution, instruction.condition, fallthrough_eip)
        }
        HandlerCall::Binary {
            handler,
            left,
            right,
        } => handler(
            execution,
            map_operand(left),
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
        HandlerCall::Ternary {
            handler,
            destination,
            first_source,
            second_source,
        } => handler(
            execution,
            map_location(destination),
            map_operand(first_source),
            map_operand(second_source),
            instruction.condition,
            fallthrough_eip,
        ),
    }
}

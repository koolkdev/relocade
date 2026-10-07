use wasm86_compiler::{BuildError, Val, I32};

use super::{handlers::HandlerCall, map_location, map_operand, Instruction, RealModeSupport};
use crate::{execution::ExecutionBuilder, ExecutionProfile};

pub(crate) fn lower(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: Instruction<impl Into<Val<I32>>>,
    fallthrough_eip: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    if matches!(execution.profile(), ExecutionProfile::Real16) {
        match instruction.real_mode {
            RealModeSupport::Supported => {}
            RealModeSupport::InvalidOpcode => {
                execution.fault_if(true, crate::Exception::InvalidOpcode)?;
                return Ok(fallthrough_eip);
            }
            RealModeSupport::Unsupported => {
                execution.unsupported_if(true, u32::from(instruction.diagnostic_opcode))?;
                return Ok(fallthrough_eip);
            }
        }
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

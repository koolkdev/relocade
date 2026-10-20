use relocade_compiler::{BuildError, Val, I32};

use super::{handlers::HandlerCall, map_location, map_operand, Instruction};
use crate::{
    execution::{CpuFeatureCheck, ExecutionBuilder},
    Exception,
};

pub(crate) fn lower(
    execution: &mut ExecutionBuilder<'_, '_>,
    instruction: Instruction<impl Into<Val<I32>>>,
    fallthrough_eip: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    match execution.cpu_feature_check(instruction.requirements.features) {
        CpuFeatureCheck::Supported => {}
        CpuFeatureCheck::Unsupported => {
            execution.fault(Exception::InvalidOpcode)?;
            return Ok(fallthrough_eip);
        }
        CpuFeatureCheck::Runtime { missing } => {
            execution.fault_if(missing, Exception::InvalidOpcode)?;
        }
    }
    if let Some(exception) = instruction
        .requirements
        .mode_fault(execution.target().profile)
    {
        execution.fault(exception)?;
        return Ok(fallthrough_eip);
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

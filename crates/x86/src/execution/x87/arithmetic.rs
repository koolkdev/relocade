//! Register forms choose operand roles before the shared arithmetic response.

use wasm86_compiler::BuildError;

use crate::{instruction::X87StackIndex, x87};

use super::{check_pending_exception, record_instruction, ExecutionBuilder};

pub(crate) enum ProductDestination {
    Top,
    Other,
}

pub(crate) fn multiply_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    other: X87StackIndex,
    destination: ProductDestination,
    pop: bool,
) -> Result<(), BuildError> {
    check_pending_exception(execution)?;
    record_instruction(execution)?;
    let (destination, source) = match destination {
        ProductDestination::Top => (0.into(), other.offset()),
        ProductDestination::Other => (other.offset(), 0.into()),
    };
    execution.state.x87.binary_register(
        &mut execution.body,
        destination,
        source,
        pop,
        x87::multiply,
    )
}

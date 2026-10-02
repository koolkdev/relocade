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
    let (destination, source) = match destination {
        ProductDestination::Top => (0.into(), other.offset()),
        ProductDestination::Other => (other.offset(), 0.into()),
    };
    let mut arithmetic = execution.state.x87.prepare_binary_register(
        &mut execution.body,
        destination,
        source,
        pop,
        x87::multiply,
    )?;
    execution.specialize(|jit| {
        jit.specialize_on(arithmetic.normal_operands())?;
        jit.specialize_on(arithmetic.in_range())?;
        // Normal operands and an in-range product permit only precision loss.
        arithmetic.assume_normal_result();
        Ok(())
    })?;
    record_instruction(execution)?;
    arithmetic.commit(&mut execution.body, &mut execution.state.x87)
}

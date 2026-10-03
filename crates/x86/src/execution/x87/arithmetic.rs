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
    )?;
    let mut product = arithmetic.calculate(x87::multiply);
    execution.specialize(|jit| {
        let candidate = product.rounding_candidate(&mut jit.body)?;
        // The candidate accepts two normal operands with an in-range product,
        // or zero with a normal/zero partner. Empty slots can retain valid bits,
        // so stack presence must also be established.
        jit.specialize_on(arithmetic.operands_present().and(candidate.valid))?;
        arithmetic.assume_present();
        product.result = x87::ArithmeticResult::from_rounding(candidate.rounded);
        Ok(())
    })?;
    record_instruction(execution)?;
    arithmetic.commit(
        &mut execution.body,
        &mut execution.state.x87,
        product.result,
    )
}

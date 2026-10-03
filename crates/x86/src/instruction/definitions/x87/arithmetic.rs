//! Register multiplication reads ST(i) before an optional pop.

use super::*;
use crate::{instruction::X87StackIndex, x87};

instruction_families! {
    FMUL_TOP {
        execute: multiply_register(ProductDestination::Top, false);
        forms { 0xD8 @ 0xC8 + rm => operands(st); }
    }
    FMUL_REGISTER {
        execute: multiply_register(ProductDestination::Other, false);
        forms { 0xDC @ 0xC8 + rm => operands(st); }
    }
    FMULP_REGISTER {
        execute: multiply_register(ProductDestination::Other, true);
        forms { 0xDE @ 0xC8 + rm => operands(st); }
    }
}

enum ProductDestination {
    Top,
    Other,
}

fn multiply_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    other: X87StackIndex,
    destination: ProductDestination,
    pop: bool,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let (destination, source) = match destination {
        ProductDestination::Top => (0.into(), other.offset()),
        ProductDestination::Other => (other.offset(), 0.into()),
    };
    let mut arithmetic = execution
        .x87()
        .prepare_binary_register(destination, source, pop)?;
    let mut product = arithmetic.calculate(x87::multiply);
    execution.specialize(|jit| {
        let candidate = jit.compute(|body| product.rounding_candidate(body))?;
        // The candidate accepts two normal operands with an in-range product,
        // or zero with a normal/zero partner. Empty slots can retain valid bits,
        // so stack presence must also be established.
        jit.specialize_on(arithmetic.operands_present().and(candidate.valid))?;
        arithmetic.assume_present();
        product.result = x87::ArithmeticResult::from_rounding(candidate.rounded);
        Ok(())
    })?;
    execution.record_x87_instruction()?;
    execution
        .x87()
        .commit_arithmetic(arithmetic, product.result)
}

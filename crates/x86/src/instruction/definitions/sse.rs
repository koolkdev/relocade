//! SSE and SSE2 instruction families share one catalog entry point.

mod arithmetic;
mod comparisons;
mod half_moves;
mod logical;
mod moves;
mod mxcsr;

use super::*;
use crate::{
    instruction::{VectorAlignment, XmmLocation},
    register::RegisterCode,
};
use wasm86_compiler::V128;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    arithmetic::forms()
        .chain(comparisons::forms())
        .chain(moves::forms())
        .chain(half_moves::forms())
        .chain(logical::forms())
        .chain(mxcsr::forms())
}

/// Legacy XMM binary operations read an aligned m128 source before updating state.
fn binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: impl FnOnce(Val<V128>, Val<V128>) -> Val<V128>,
) -> Result<(), BuildError> {
    let right = source.read::<V128>(execution, VectorAlignment::Aligned)?;
    let left = execution.read_xmm(destination.clone())?;
    execution.write_xmm(destination, operation(left, right))
}

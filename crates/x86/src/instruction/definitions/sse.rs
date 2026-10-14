//! SSE and SSE2 instruction families share one catalog entry point.

mod arithmetic;
mod average;
mod comparisons;
mod convert_float_width;
mod convert_from_integer;
mod convert_mmx;
mod convert_packed_integer;
mod convert_scalar_integer;
mod float_add;
mod float_compare;
mod float_divide;
mod float_minmax;
mod float_multiply;
mod float_sqrt;
mod floating;
mod half_moves;
mod logical;
mod masks;
mod minmax;
mod moves;
mod multiply;
mod mxcsr;
mod pack;
mod saturation;
mod shifts;
mod shuffle;
mod sum_differences;
mod unpack;
mod words;

use super::*;
use crate::{
    instruction::{VectorAlignment, XmmLocation},
    register::RegisterCode,
};
use wasm86_compiler::V128;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    arithmetic::forms()
        .chain(comparisons::forms())
        .chain(convert_float_width::forms())
        .chain(convert_from_integer::forms())
        .chain(convert_mmx::forms())
        .chain(convert_packed_integer::forms())
        .chain(convert_scalar_integer::forms())
        .chain(float_add::forms())
        .chain(float_compare::forms())
        .chain(float_divide::forms())
        .chain(float_minmax::forms())
        .chain(float_multiply::forms())
        .chain(float_sqrt::forms())
        .chain(moves::forms())
        .chain(average::forms())
        .chain(minmax::forms())
        .chain(multiply::forms())
        .chain(half_moves::forms())
        .chain(logical::forms())
        .chain(masks::forms())
        .chain(mxcsr::forms())
        .chain(pack::forms())
        .chain(saturation::forms())
        .chain(shifts::forms())
        .chain(sum_differences::forms())
        .chain(shuffle::forms())
        .chain(unpack::forms())
        .chain(words::forms())
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

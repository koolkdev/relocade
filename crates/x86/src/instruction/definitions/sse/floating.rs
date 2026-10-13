//! Shared operand access and precise result publication for SIMD numerical operations.

use super::*;
use crate::{
    instruction::XmmType,
    simd::{FloatLane, FloatOperands, FloatResult, FloatShape},
};
use wasm86_compiler::{AtLeast, I64};

pub(super) fn binary<T: FloatLane + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
    operation: impl Fn(FloatOperands<T>) -> FloatResult<T>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let right = match shape {
        FloatShape::Scalar => {
            Val::<V128>::splat(source.read::<T>(execution, VectorAlignment::Unaligned)?)
        }
        FloatShape::Packed => source.read::<V128>(execution, VectorAlignment::Aligned)?,
    };
    let left = execution.read_xmm(destination.clone())?;
    let controls = execution.read_mxcsr()?;
    let result = shape.apply(left, right, controls.denormals_are_zero(), operation);
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}

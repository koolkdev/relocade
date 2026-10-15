//! Arithmetic handlers establish controls before typed access and result publication.

use super::*;
use crate::simd::{ArithmeticOperands, FloatArithmetic, FloatOperand};
use wasm86_compiler::{AtLeast, I64};

pub(in crate::instruction::definitions::sse) fn scalar_arithmetic<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: FloatArithmetic,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    scalar_binary::<T>(execution, destination, source, |execution, left, right| {
        execution.calculate(|body| {
            ArithmeticOperands::new(left, right, controls.denormals_are_zero())
                .calculate(body, operation, &controls)
        })
    })
}

pub(in crate::instruction::definitions::sse) fn packed_arithmetic<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: FloatArithmetic,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    packed_binary(execution, destination, source, |execution, left, right| {
        packed_lanes::<T>(execution, left, right, |body, left, right| {
            ArithmeticOperands::new(left, right, controls.denormals_are_zero())
                .calculate(body, operation, &controls)
        })
    })
}

pub(in crate::instruction::definitions::sse) fn scalar_square_root<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    scalar_binary::<T>(execution, destination, source, |execution, _, right| {
        execution.calculate(|body| {
            FloatOperand::new(right, controls.denormals_are_zero()).square_root(body, &controls)
        })
    })
}

pub(in crate::instruction::definitions::sse) fn packed_square_root<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    packed_binary(execution, destination, source, |execution, left, right| {
        packed_lanes::<T>(execution, left, right, |body, _, right| {
            FloatOperand::new(right, controls.denormals_are_zero()).square_root(body, &controls)
        })
    })
}

//! Arithmetic handlers establish controls before typed access and result publication.

use super::*;
use crate::simd::{
    ArithmeticOperands, FloatArithmetic, FloatOperand, PackedFloatCandidate, PackedFloatOperand,
};
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
    let controls = execution.simd_arithmetic_controls()?;
    scalar_binary::<T>(execution, destination, source, |execution, left, right| {
        let specialized = execution.specialize_simd_arithmetic(|| {
            arithmetic_candidate::<T>(
                &controls,
                Val::<V128>::splat(left.clone()),
                Val::<V128>::splat(right.clone()),
                operation,
            )
        })?;
        match specialized {
            Some(result) => Ok(FloatResult {
                value: result.value.extract_lane::<T>(0),
                exceptions: result.exceptions,
            }),
            None => execution.calculate(|body| {
                ArithmeticOperands::new(left, right, controls.denormals_are_zero())
                    .calculate(body, operation, &controls)
            }),
        }
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
    let controls = execution.simd_arithmetic_controls()?;
    packed_binary(execution, destination, source, |execution, left, right| {
        let specialized = execution.specialize_simd_arithmetic(|| {
            arithmetic_candidate::<T>(&controls, left.clone(), right.clone(), operation)
        })?;
        match specialized {
            Some(result) => Ok(result),
            None => packed_lanes::<T>(execution, left, right, |body, left, right| {
                ArithmeticOperands::new(left, right, controls.denormals_are_zero())
                    .calculate(body, operation, &controls)
            }),
        }
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
    let controls = execution.simd_arithmetic_controls()?;
    scalar_binary::<T>(execution, destination, source, |execution, _, right| {
        let specialized = execution.specialize_simd_arithmetic(|| {
            square_root_candidate::<T>(&controls, Val::<V128>::splat(right.clone()))
        })?;
        match specialized {
            Some(result) => Ok(FloatResult {
                value: result.value.extract_lane::<T>(0),
                exceptions: result.exceptions,
            }),
            None => execution.calculate(|body| {
                FloatOperand::new(right, controls.denormals_are_zero()).square_root(body, &controls)
            }),
        }
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
    let controls = execution.simd_arithmetic_controls()?;
    packed_binary(execution, destination, source, |execution, left, right| {
        let specialized = execution
            .specialize_simd_arithmetic(|| square_root_candidate::<T>(&controls, right.clone()))?;
        match specialized {
            Some(result) => Ok(result),
            None => packed_lanes::<T>(execution, left, right, |body, _, right| {
                FloatOperand::new(right, controls.denormals_are_zero()).square_root(body, &controls)
            }),
        }
    })
}

fn arithmetic_candidate<T: BinaryEncoding>(
    controls: &Mxcsr,
    left: Val<V128>,
    right: Val<V128>,
    operation: FloatArithmetic,
) -> PackedFloatCandidate {
    let left = PackedFloatOperand::new(T::FORMAT, left, controls.denormals_are_zero());
    let right = PackedFloatOperand::new(T::FORMAT, right, controls.denormals_are_zero());
    left.arithmetic_nearest(&right, operation)
}

fn square_root_candidate<T: BinaryEncoding>(
    controls: &Mxcsr,
    source: Val<V128>,
) -> PackedFloatCandidate {
    let source = PackedFloatOperand::new(T::FORMAT, source, controls.denormals_are_zero());
    source.square_root_nearest()
}

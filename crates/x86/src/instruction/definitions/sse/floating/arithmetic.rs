//! Arithmetic handlers establish controls before typed access and result publication.

use super::*;
use crate::simd::{ArithmeticOperands, FloatArithmetic, FloatOperand, PackedFloatOperand};
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
    let native = execution.specialize_simd_nearest(&controls)?;
    scalar_binary::<T>(execution, destination, source, |execution, left, right| {
        if native {
            let result = native_arithmetic::<T>(
                execution,
                &controls,
                Val::<V128>::splat(left),
                Val::<V128>::splat(right),
                operation,
            )?;
            return Ok(FloatResult {
                value: result.value.extract_lane::<T>(0),
                exceptions: result.exceptions,
            });
        }
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
    let native = execution.specialize_simd_nearest(&controls)?;
    packed_binary(execution, destination, source, |execution, left, right| {
        if native {
            return native_arithmetic::<T>(execution, &controls, left, right, operation);
        }
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
    let native = execution.specialize_simd_nearest(&controls)?;
    scalar_binary::<T>(execution, destination, source, |execution, _, right| {
        if native {
            let result = native_square_root::<T>(execution, &controls, Val::<V128>::splat(right))?;
            return Ok(FloatResult {
                value: result.value.extract_lane::<T>(0),
                exceptions: result.exceptions,
            });
        }
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
    let native = execution.specialize_simd_nearest(&controls)?;
    packed_binary(execution, destination, source, |execution, left, right| {
        if native {
            return native_square_root::<T>(execution, &controls, right);
        }
        packed_lanes::<T>(execution, left, right, |body, _, right| {
            FloatOperand::new(right, controls.denormals_are_zero()).square_root(body, &controls)
        })
    })
}

fn native_arithmetic<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    controls: &Mxcsr,
    left: Val<V128>,
    right: Val<V128>,
    operation: FloatArithmetic,
) -> Result<FloatResult<V128>, BuildError> {
    let left = PackedFloatOperand::new(T::FORMAT, left, controls.denormals_are_zero());
    let right = PackedFloatOperand::new(T::FORMAT, right, controls.denormals_are_zero());
    let candidate = left.arithmetic_nearest(&right, operation);
    execution.specialize_on(candidate.valid)?;
    Ok(candidate.result)
}

fn native_square_root<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    controls: &Mxcsr,
    source: Val<V128>,
) -> Result<FloatResult<V128>, BuildError> {
    let source = PackedFloatOperand::new(T::FORMAT, source, controls.denormals_are_zero());
    let candidate = source.square_root_nearest();
    execution.specialize_on(candidate.valid)?;
    Ok(candidate.result)
}

//! Shared operand access and precise result publication for SIMD numerical operations.

use super::*;
use crate::{
    floating::{BinaryEncoding, BinaryFormat, RoundingMode},
    instruction::XmmType,
    simd::{
        ArithmeticOperands, FloatArithmetic, FloatOperand, FloatOperands, FloatResult, FloatShape,
    },
    state::Mxcsr,
};
use wasm86_compiler::{AtLeast, BlockBuilder, I64};

#[derive(Clone, Copy)]
pub(super) enum IntegerRounding {
    Mxcsr,
    Truncate,
}

impl IntegerRounding {
    pub(super) fn mode(self, controls: &Mxcsr) -> RoundingMode {
        match self {
            Self::Mxcsr => controls.rounding(),
            Self::Truncate => RoundingMode::new(3.into()),
        }
    }
}

pub(super) fn rounding_for_integer_source(
    execution: &mut ExecutionBuilder<'_, '_>,
    format: BinaryFormat,
) -> Result<RoundingMode, BuildError> {
    // Every signed dword is exactly representable in binary64, so that path
    // has no MXCSR dependency. Only binary32 conversion needs observed controls.
    Ok(match format {
        BinaryFormat::Binary32 => execution.simd_controls()?.rounding(),
        BinaryFormat::Binary64 => RoundingMode::new(0.into()),
    })
}

pub(super) fn binary<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
    operation: impl Fn(FloatOperands<T>) -> FloatResult<T>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    calculate_lanes::<T>(execution, destination, source, shape, |_, left, right| {
        Ok(operation(FloatOperands::new(
            left,
            right,
            controls.denormals_are_zero(),
        )))
    })
}

/// Numerical arithmetic can build exact alignment and refinement branches. All
/// lanes are evaluated before status or the destination is published.
pub(super) fn arithmetic<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
    operation: FloatArithmetic,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    calculate_lanes::<T>(
        execution,
        destination,
        source,
        shape,
        |body, left, right| {
            ArithmeticOperands::new(left, right, controls.denormals_are_zero())
                .calculate(body, operation, &controls)
        },
    )
}

pub(super) fn square_root<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    calculate_lanes::<T>(execution, destination, source, shape, |body, _, right| {
        FloatOperand::new(right, controls.denormals_are_zero()).square_root(body, &controls)
    })
}

/// Approximate single-precision operations have fixed denormal and rounding
/// behavior and never raise SIMD floating-point exceptions.
pub(super) fn approximate(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
    operation: impl Fn(Val<I32>) -> Val<I32>,
) -> Result<(), BuildError> {
    calculate_lanes::<I32>(execution, destination, source, shape, |_, _, right| {
        Ok(FloatResult {
            value: operation(right),
            exceptions: Default::default(),
        })
    })
}

fn calculate_lanes<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
    mut calculate: impl FnMut(
        &mut BlockBuilder<'_>,
        Val<T>,
        Val<T>,
    ) -> Result<FloatResult<T>, BuildError>,
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
    let count = match shape {
        FloatShape::Scalar => 1,
        FloatShape::Packed => 16 / T::FORMAT.bytes(),
    };
    let result = FloatResult::try_from_lanes(left.clone(), count, |lane| {
        execution.calculate(|body| {
            calculate(
                body,
                left.extract_lane::<T>(lane),
                right.extract_lane::<T>(lane),
            )
        })
    })?;
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}

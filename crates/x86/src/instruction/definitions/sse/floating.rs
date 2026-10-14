//! Shared operand access and precise result publication for SIMD numerical operations.

use super::*;
use crate::{
    floating::{BinaryEncoding, BinaryFormat, RoundingMode},
    instruction::XmmType,
    simd::{
        ArithmeticOperands, FloatArithmetic, FloatOperand, FloatOperands, FloatResult,
        PackedFloatOperand,
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

pub(super) fn scalar_binary<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: impl FnOnce(FloatOperands<T>) -> FloatResult<T>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    let controls = execution.simd_controls()?;
    let right = source.read::<T>(execution, VectorAlignment::Unaligned)?;
    let left = T::read_register(execution, destination.clone())?;
    let result = operation(FloatOperands::new(
        left,
        right,
        controls.denormals_are_zero(),
    ));
    execution.record_simd_exceptions(&result.exceptions)?;
    T::write_register(execution, destination, result.value)
}

pub(super) fn packed_binary<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    operation: impl FnOnce(PackedFloatOperand, PackedFloatOperand) -> FloatResult<V128>,
) -> Result<(), BuildError> {
    let controls = execution.simd_controls()?;
    calculate_vectors::<T>(
        execution,
        destination,
        source,
        FloatShape::Packed,
        |_, left, right| {
            Ok(operation(
                PackedFloatOperand::new(T::FORMAT, left, controls.denormals_are_zero()),
                PackedFloatOperand::new(T::FORMAT, right, controls.denormals_are_zero()),
            ))
        },
    )
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
    if execution.specialize_simd_nearest(&controls)? {
        return calculate_vectors::<T>(
            execution,
            destination,
            source,
            shape,
            |execution, left, right| {
                let left = PackedFloatOperand::new(T::FORMAT, left, controls.denormals_are_zero());
                let right =
                    PackedFloatOperand::new(T::FORMAT, right, controls.denormals_are_zero());
                let candidate = left.arithmetic_nearest(&right, operation);
                execution.specialize_on(candidate.valid)?;
                Ok(candidate.result)
            },
        );
    }
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
    if execution.specialize_simd_nearest(&controls)? {
        return calculate_vectors::<T>(
            execution,
            destination,
            source,
            shape,
            |execution, _, right| {
                let candidate =
                    PackedFloatOperand::new(T::FORMAT, right, controls.denormals_are_zero())
                        .square_root_nearest();
                execution.specialize_on(candidate.valid)?;
                Ok(candidate.result)
            },
        );
    }
    calculate_lanes::<T>(execution, destination, source, shape, |body, _, right| {
        FloatOperand::new(right, controls.denormals_are_zero()).square_root(body, &controls)
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
    let count = match shape {
        FloatShape::Scalar => 1,
        FloatShape::Packed => 16 / T::FORMAT.bytes(),
    };
    calculate_vectors::<T>(
        execution,
        destination,
        source,
        shape,
        |execution, left, right| {
            FloatResult::try_from_lanes(0_u128.into(), count, |lane| {
                execution.calculate(|body| {
                    calculate(
                        body,
                        left.extract_lane::<T>(lane),
                        right.extract_lane::<T>(lane),
                    )
                })
            })
        },
    )
}

/// Operand access and publication are shared by whole-vector and per-lane calculations.
/// Scalar operands are splatted before calculation; only the low result lane is
/// published. Unused destination bits never participate in numerical operations.
fn calculate_vectors<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    shape: FloatShape,
    calculate: impl FnOnce(
        &mut ExecutionBuilder<'_, '_>,
        Val<V128>,
        Val<V128>,
    ) -> Result<FloatResult<V128>, BuildError>,
) -> Result<(), BuildError> {
    let right = match shape {
        FloatShape::Scalar => {
            Val::<V128>::splat(source.read::<T>(execution, VectorAlignment::Unaligned)?)
        }
        FloatShape::Packed => source.read::<V128>(execution, VectorAlignment::Aligned)?,
    };
    let original = execution.read_xmm(destination.clone())?;
    let left = match shape {
        FloatShape::Scalar => Val::<V128>::splat(original.extract_lane::<T>(0)),
        FloatShape::Packed => original.clone(),
    };
    let result = calculate(execution, left, right)?;
    execution.record_simd_exceptions(&result.exceptions)?;
    let value = match shape {
        FloatShape::Scalar => original.replace_lane(0, result.value.extract_lane::<T>(0)),
        FloatShape::Packed => result.value,
    };
    execution.write_xmm(destination, value)
}

#[derive(Clone, Copy)]
pub(super) enum FloatShape {
    Scalar,
    Packed,
}

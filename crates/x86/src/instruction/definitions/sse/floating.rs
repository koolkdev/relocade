//! Typed operand access and precise publication for SSE floating-point instructions.

mod arithmetic;

pub(super) use arithmetic::{
    packed_arithmetic, packed_square_root, scalar_arithmetic, scalar_square_root,
};

use super::*;
use crate::{
    floating::{BinaryEncoding, BinaryFormat, RoundingMode},
    instruction::XmmType,
    simd::FloatResult,
    state::Mxcsr,
};
use wasm86_compiler::BlockBuilder;

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

/// Callers establish numerical controls and specialization before operand access.
/// Scalar calculations see only the active lane and preserve the other XMM bits.
pub(super) fn scalar_binary<T: BinaryEncoding + XmmType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    calculate: impl FnOnce(
        &mut ExecutionBuilder<'_, '_>,
        Val<T>,
        Val<T>,
    ) -> Result<FloatResult<T>, BuildError>,
) -> Result<(), BuildError> {
    let right = source.read::<T>(execution, VectorAlignment::Unaligned)?;
    let left = T::read_register(execution, destination.clone())?;
    let result = calculate(execution, left, right)?;
    execution.record_simd_exceptions(&result.exceptions)?;
    T::write_register(execution, destination, result.value)
}

/// Packed calculations collect all lane exceptions before publishing the result.
pub(super) fn packed_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: XmmLocation,
    calculate: impl FnOnce(
        &mut ExecutionBuilder<'_, '_>,
        Val<V128>,
        Val<V128>,
    ) -> Result<FloatResult<V128>, BuildError>,
) -> Result<(), BuildError> {
    let right = source.read::<V128>(execution, VectorAlignment::Aligned)?;
    let left = execution.read_xmm(destination.clone())?;
    let result = calculate(execution, left, right)?;
    execution.record_simd_exceptions(&result.exceptions)?;
    execution.write_xmm(destination, result.value)
}

pub(super) fn packed_lanes<T: BinaryEncoding>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: Val<V128>,
    right: Val<V128>,
    mut calculate: impl FnMut(
        &mut BlockBuilder<'_>,
        Val<T>,
        Val<T>,
    ) -> Result<FloatResult<T>, BuildError>,
) -> Result<FloatResult<V128>, BuildError> {
    FloatResult::try_from_lanes(left.clone(), 16 / T::FORMAT.bytes(), |lane| {
        execution.calculate(|body| {
            calculate(
                body,
                left.extract_lane::<T>(lane),
                right.extract_lane::<T>(lane),
            )
        })
    })
}

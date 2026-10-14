//! Shared operand access and precise result publication for SIMD numerical operations.

use super::*;
use crate::{
    floating::{BinaryEncoding, RoundingMode},
    instruction::XmmType,
    simd::{FloatOperands, FloatResult, FloatShape},
    state::Mxcsr,
};
use wasm86_compiler::{AtLeast, I64};

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

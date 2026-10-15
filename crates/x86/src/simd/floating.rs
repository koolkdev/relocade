//! Floating-point calculations report numerical conditions without changing state.

mod arithmetic;
mod compare;
mod estimate;
mod exceptions;
mod integer;
mod operand;
mod packed;
mod rounding;
mod sqrt;
mod width;
pub(crate) use arithmetic::{ArithmeticOperands, FloatArithmetic};
pub(crate) use compare::{FloatMinMax, FloatOperands};
pub(crate) use estimate::FloatEstimate;
pub(crate) use exceptions::SimdExceptions;
pub(crate) use integer::integer_to_float;
pub(crate) use operand::FloatOperand;
pub(crate) use packed::{estimate_packed_float, packed_integer_to_float, PackedFloatOperand};

use wasm86_compiler::{Val, ValueType, VectorLane, V128};

pub(crate) struct FloatResult<T: ValueType> {
    pub(crate) value: Val<T>,
    pub(crate) exceptions: SimdExceptions,
}

impl FloatResult<V128> {
    /// Replace result lanes and collect every condition before any state update.
    /// The initial vector determines which unused destination bits are retained.
    pub(crate) fn from_lanes<T: VectorLane>(
        initial: Val<V128>,
        count: u32,
        operation: impl Fn(u32) -> FloatResult<T>,
    ) -> Self {
        Self::try_from_lanes(initial, count, |lane| {
            Ok::<_, std::convert::Infallible>(operation(lane))
        })
        .unwrap_or_else(|never| match never {})
    }

    /// Calculations that build compiler branches can fail before producing a lane.
    pub(crate) fn try_from_lanes<T: VectorLane, E>(
        initial: Val<V128>,
        count: u32,
        mut operation: impl FnMut(u32) -> Result<FloatResult<T>, E>,
    ) -> Result<Self, E> {
        let mut result = Self {
            value: initial,
            exceptions: SimdExceptions::default(),
        };
        for lane in 0..count {
            let lane_result = operation(lane)?;
            result.value = result.value.replace_lane(lane, lane_result.value);
            result.exceptions.merge(lane_result.exceptions);
        }
        Ok(result)
    }
}

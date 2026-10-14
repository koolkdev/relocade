//! Floating-point calculations report numerical conditions without changing state.

mod compare;
mod exceptions;
mod integer;
mod operand;
mod rounding;
mod width;
pub(crate) use compare::FloatOperands;
pub(crate) use exceptions::SimdExceptions;
pub(crate) use integer::integer_to_float;
pub(crate) use operand::FloatOperand;

use crate::floating::BinaryEncoding;
use wasm86_compiler::{AtLeast, Val, ValueType, VectorLane, I1, I64, V128};

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
        let mut result = Self {
            value: initial,
            exceptions: SimdExceptions::default(),
        };
        for lane in 0..count {
            let lane_result = operation(lane);
            result.value = result.value.replace_lane(lane, lane_result.value);
            result.exceptions.merge(lane_result.exceptions);
        }
        result
    }
}

#[derive(Clone, Copy)]
pub(crate) enum FloatShape {
    Scalar,
    Packed,
}

impl FloatShape {
    /// Scalar operations retain upper lanes. Packed operations collect conditions
    /// from every lane, even when one of them will cause an unmasked exception.
    pub(crate) fn apply<T: BinaryEncoding>(
        self,
        left: Val<V128>,
        right: Val<V128>,
        daz: Val<I1>,
        operation: impl Fn(FloatOperands<T>) -> FloatResult<T>,
    ) -> FloatResult<V128>
    where
        I64: AtLeast<T>,
    {
        let lanes = match self {
            Self::Scalar => 1,
            Self::Packed => 16 / T::FORMAT.bytes(),
        };
        FloatResult::from_lanes(left.clone(), lanes, |lane| {
            let operands = FloatOperands::new(
                left.extract_lane(lane),
                right.extract_lane(lane),
                daz.clone(),
            );
            operation(operands)
        })
    }
}

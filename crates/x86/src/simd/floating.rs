//! Floating-point calculations report numerical conditions without changing state.

mod compare;
mod exceptions;
mod integer;
mod operand;
pub(crate) use compare::FloatOperands;
pub(crate) use exceptions::SimdExceptions;
pub(crate) use operand::{FloatLane, FloatOperand};

use wasm86_compiler::{AtLeast, Val, ValueType, I1, I64, V128};

pub(crate) struct FloatResult<T: ValueType> {
    pub(crate) value: Val<T>,
    pub(crate) exceptions: SimdExceptions,
}

#[derive(Clone, Copy)]
pub(crate) enum FloatShape {
    Scalar,
    Packed,
}

impl FloatShape {
    /// Scalar operations retain upper lanes. Packed operations collect conditions
    /// from every lane, even when one of them will cause an unmasked exception.
    pub(crate) fn apply<T: FloatLane>(
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
        let mut result = FloatResult {
            value: left.clone(),
            exceptions: SimdExceptions::default(),
        };
        for lane in 0..lanes {
            let operands = FloatOperands::new(
                left.extract_lane(lane),
                right.extract_lane(lane),
                daz.clone(),
            );
            let lane_result = operation(operands);
            result.value = result.value.replace_lane(lane, lane_result.value);
            result.exceptions.merge(lane_result.exceptions);
        }
        result
    }
}

//! Pure x86 SIMD value operations for MMX and SSE.
//! Operand access and architectural state effects belong to instruction execution.

mod floating;
mod shifts;
mod shuffle;
mod sum_differences;
mod unpack;

pub(crate) use floating::{
    estimate_packed_float, integer_to_float, narrow_packed_float, packed_integer_to_float,
    widen_packed_float, ArithmeticOperands, FloatArithmetic, FloatEstimate, FloatMinMax,
    FloatOperand, FloatOperands, FloatResult, PackedFloatCandidate, PackedFloatOperand,
    SimdExceptions,
};
pub(crate) use shifts::{ByteShiftOp, PackedShiftOp};
pub(crate) use shuffle::{shuffle_dwords, shuffle_qwords, shuffle_words};
pub(crate) use sum_differences::sum_absolute_byte_differences;
pub(crate) use unpack::unpack_lanes;

//! Pure x86 SIMD value operations shared by MMX and SSE.
//! Operand access and architectural state effects belong to instruction execution.

mod shifts;
mod shuffle;
mod unpack;

pub(crate) use shifts::{ByteShiftOp, PackedShiftOp};
pub(crate) use shuffle::{shuffle_dwords, shuffle_qwords, shuffle_words};
pub(crate) use unpack::unpack_lanes;

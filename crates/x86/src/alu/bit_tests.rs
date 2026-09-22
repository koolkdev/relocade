//! Bit selection, optional modification and the old bit's carry flag.

use wasm86_compiler::{MemoryInt, Val, I1, I32};

use super::AluResult;
use crate::flags::{FlagChange, StatusFlag};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum BitTestOp {
    Test,
    Set,
    Reset,
    Complement,
}

impl BitTestOp {
    pub(crate) fn mask<T: MemoryInt>(offset: &Val<I32>) -> Val<T> {
        Val::<T>::from(1).shl(offset.and(T::BYTES * 8 - 1))
    }

    /// Offsets wrap within this logical operand. Selecting another memory unit
    /// from a signed bit-string offset belongs to the instruction's addressing.
    pub(crate) fn apply<T: MemoryInt>(self, input: Val<T>, offset: Val<I32>) -> AluResult<T> {
        let index = offset.and(T::BYTES * 8 - 1);
        let carry = input.unsigned().shr(&index).truncate::<I1>();
        let mask = Self::mask::<T>(&offset);
        let result = match self {
            Self::Test => input,
            Self::Set => input.or(mask),
            Self::Reset => input.and(mask.xor(-1)),
            Self::Complement => input.xor(mask),
        };
        // ZF is unchanged; OF/SF/AF/PF are undefined. Preserve all five.
        let flags = FlagChange::partial([(StatusFlag::CF.into(), carry)]);
        AluResult { result, flags }
    }
}

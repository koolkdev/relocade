//! Logical-width rotations and their carry/overflow changes.

use wasm86_compiler::{MemoryInt, Val, I32};

use super::{bit, FlagChange, StatusFlag};

#[derive(Clone, Copy)]
pub(crate) enum RotateKind {
    Left,
    Right,
}

pub(crate) struct RotateResult<T: MemoryInt> {
    pub(crate) result: Val<T>,
    pub(crate) flags: FlagChange,
}

impl RotateKind {
    /// Count is masked to five bits; the caller applies flags only when nonzero.
    pub(crate) fn apply<T: MemoryInt>(self, input: Val<T>, count: Val<I32>) -> RotateResult<T> {
        let width = T::BYTES * 8;
        let result = match self {
            Self::Left => input.rotl(&count),
            Self::Right => input.rotr(&count),
        };
        let carry = match self {
            Self::Left => bit(&result, 0),
            Self::Right => bit(&result, width - 1),
        };
        let overflow = match self {
            Self::Left => bit(&result, width - 1).xor(&carry),
            Self::Right => bit(&result, width - 1).xor(bit(&result, width - 2)),
        }
        .and(count.eq(1));
        // OF is undefined when the masked count exceeds one; choose zero.
        let flags = FlagChange::partial([(StatusFlag::CF, carry), (StatusFlag::OF, overflow)]);
        RotateResult { result, flags }
    }
}

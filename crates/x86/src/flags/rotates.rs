//! Operand and through-carry rotations with their carry/overflow changes.

use wasm86_compiler::{AtLeast, MemoryInt, Val, I1, I32, I64};

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
    pub(crate) fn plain<T: MemoryInt>(self, input: Val<T>, count: Val<I32>) -> RotateResult<T> {
        let width = T::BYTES * 8;
        let result = match self {
            Self::Left => input.rotl(&count),
            Self::Right => input.rotr(&count),
        };
        let carry = match self {
            Self::Left => bit(&result, 0),
            Self::Right => bit(&result, width - 1),
        };
        self.result_with_flags(result, carry, &count)
    }

    /// Rotates the operand and incoming CF using a count masked to five bits.
    /// The caller applies flags only when that masked count is nonzero.
    pub(crate) fn through_carry<T: MemoryInt>(
        self,
        input: Val<T>,
        count: Val<I32>,
        carry: Val<I1>,
    ) -> RotateResult<T>
    where
        I32: AtLeast<T>,
    {
        let width = T::BYTES * 8;
        let effective = carry_count(count.clone(), width + 1);
        let value = input.unsigned().extend::<I32>();
        let (rotated, rotated_carry) = if width == 32 {
            let ring = value
                .unsigned()
                .extend::<I64>()
                .or(carry.unsigned().extend::<I64>().shl(width));
            let rotated = self.rotate_ring(ring, width + 1, &effective);
            (
                rotated.truncate::<I32>().truncate::<T>(),
                bit(&rotated, width),
            )
        } else {
            let ring = value.or(carry.unsigned().extend::<I32>().shl(width));
            let rotated = self.rotate_ring(ring, width + 1, &effective);
            (rotated.truncate::<T>(), bit(&rotated, width))
        };
        let moved = effective.ne(0);
        let result = moved.select(rotated, input);
        let carry = moved.select(rotated_carry, carry);
        self.result_with_flags(result, carry, &count)
    }

    fn rotate_ring<T: MemoryInt>(self, ring: Val<T>, width: u32, count: &Val<I32>) -> Val<T> {
        let back = Val::<I32>::from(width).sub(count);
        // The ring is narrower than its carrier, so zero counts are also valid.
        // Only the low ring bits are observed by the result and carry consumers.
        match self {
            Self::Left => ring.shl(count).or(ring.unsigned().shr(back)),
            Self::Right => ring.unsigned().shr(count).or(ring.shl(back)),
        }
    }

    fn result_with_flags<T: MemoryInt>(
        self,
        result: Val<T>,
        carry: Val<I1>,
        count: &Val<I32>,
    ) -> RotateResult<T> {
        let width = T::BYTES * 8;
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

fn carry_count(mut count: Val<I32>, ring_width: u32) -> Val<I32> {
    // A masked count is at most 31: subtract 18 then 9 for a byte ring,
    // subtract 17 for a word ring, and leave a dword count unchanged.
    for multiple in [2 * ring_width, ring_width] {
        if multiple < 32 {
            count = count
                .unsigned()
                .ge(multiple)
                .select(count.sub(multiple), count);
        }
    }
    count
}

//! Operand and through-carry rotations with their carry/overflow changes.

use wasm86_compiler::{AtLeast, MemoryInt, Val, I1, I32, I64};

use super::{bit, AluResult};
use crate::flags::{FlagChange, StatusFlag};

#[derive(Clone, Copy)]
pub(crate) enum RotateDirection {
    Left,
    Right,
}

impl RotateDirection {
    /// Count is masked to five bits; a nonzero masked count changes CF and OF.
    pub(crate) fn rotate<T: MemoryInt>(self, input: Val<T>, count: Val<I32>) -> AluResult<T> {
        let width = T::BYTES * 8;
        let result = match self {
            Self::Left => input.rotl(&count),
            Self::Right => input.rotr(&count),
        };
        let carry = match self {
            Self::Left => bit(&result, 0),
            Self::Right => bit(&result, width - 1),
        };
        AluResult {
            flags: self.flags(&result, carry, &count).when(count.ne(0)),
            result,
        }
    }

    /// Rotates the operand and incoming CF using a count masked to five bits.
    /// A zero effective count preserves the operand and entire flag source.
    pub(crate) fn rotate_through_carry<T: MemoryInt>(
        self,
        input: Val<T>,
        count: Val<I32>,
        carry: Val<I1>,
    ) -> AluResult<T>
    where
        I32: AtLeast<T>,
    {
        let width = T::BYTES * 8;
        let effective = reduce_carry_count(count.clone(), width + 1);
        let extended_input = input.unsigned().extend::<I32>();
        let (rotated, rotated_carry) = if width == 32 {
            let ring = extended_input
                .unsigned()
                .extend::<I64>()
                .or(carry.unsigned().extend::<I64>().shl(width));
            let rotated = self.rotate_ring(ring, width + 1, &effective);
            (
                rotated.truncate::<I32>().truncate::<T>(),
                bit(&rotated, width),
            )
        } else {
            let ring = extended_input.or(carry.unsigned().extend::<I32>().shl(width));
            let rotated = self.rotate_ring(ring, width + 1, &effective);
            (rotated.truncate::<T>(), bit(&rotated, width))
        };
        let has_rotation = effective.ne(0);
        let result = has_rotation.select(rotated, input);
        let carry = has_rotation.select(rotated_carry, carry);
        AluResult {
            flags: self.flags(&result, carry, &count).when(has_rotation),
            result,
        }
    }

    fn rotate_ring<T: MemoryInt>(self, ring: Val<T>, width: u32, count: &Val<I32>) -> Val<T> {
        let wrap_count = Val::<I32>::from(width).sub(count);
        // The ring is narrower than its carrier, so zero counts are also valid.
        // Only the low ring bits are observed by the result and carry consumers.
        match self {
            Self::Left => ring.shl(count).or(ring.unsigned().shr(wrap_count)),
            Self::Right => ring.unsigned().shr(count).or(ring.shl(wrap_count)),
        }
    }

    fn flags<T: MemoryInt>(self, result: &Val<T>, carry: Val<I1>, count: &Val<I32>) -> FlagChange {
        let width = T::BYTES * 8;
        let overflow = match self {
            Self::Left => bit(result, width - 1).xor(&carry),
            Self::Right => bit(result, width - 1).xor(bit(result, width - 2)),
        }
        .and(count.eq(1));
        // When flags change, OF is undefined above masked count one; choose zero.
        FlagChange::partial([
            (StatusFlag::CF.into(), carry),
            (StatusFlag::OF.into(), overflow),
        ])
    }
}

fn reduce_carry_count(mut count: Val<I32>, ring_width: u32) -> Val<I32> {
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

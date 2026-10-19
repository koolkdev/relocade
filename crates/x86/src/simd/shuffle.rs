//! Expands x86 immediate lane selectors into byte indices for `shuffle_bytes`.
//!
//! Multiplication repeats the lane's starting byte in a packed scalar: dword
//! selector 2 gives [8, 8, 8, 8], then adding byte positions gives [8, 9, 10, 11].
//! All indices stay below 32, so no carries cross byte boundaries.
//! The compiler folds this calculation for snapshot immediates; runtime decoding
//! evaluates it with the instruction's control byte.

use relocade_compiler::{Val, VectorType, I16, I32, I64, I8, V128};

/// Selects one qword from each input, ignoring the upper six control bits.
pub(crate) fn shuffle_qwords(left: Val<V128>, right: Val<V128>, control: Val<I8>) -> Val<V128> {
    let control = control.unsigned().extend::<I64>();
    let mut indices = Val::<V128>::from(0_u128);
    for lane in 0_u32..2 {
        let selector = control.unsigned().shr(lane).and(1);
        let source_byte_positions = if lane == 0 {
            [0, 1, 2, 3, 4, 5, 6, 7]
        } else {
            [16, 17, 18, 19, 20, 21, 22, 23]
        };
        let byte_indices = selector
            .mul(u64::from_le_bytes([8; 8]))
            .add(u64::from_le_bytes(source_byte_positions));
        indices = indices.replace_lane(lane, byte_indices);
    }
    left.shuffle_bytes(right, indices)
}

/// Selects four dwords, using the low and high pairs from the respective inputs.
/// Supplying the same input twice permutes its four dwords.
pub(crate) fn shuffle_dwords(left: Val<V128>, right: Val<V128>, control: Val<I8>) -> Val<V128> {
    let control = control.unsigned().extend::<I32>();
    let mut indices = Val::<V128>::from(0_u128);
    for lane in 0_u32..4 {
        let selector = control.unsigned().shr(2 * lane).and(3);
        let source_byte_positions = if lane < 2 {
            [0, 1, 2, 3]
        } else {
            [16, 17, 18, 19]
        };
        let byte_indices = selector
            .mul(u32::from_le_bytes([4; 4]))
            .add(u32::from_le_bytes(source_byte_positions));
        indices = indices.replace_lane(lane, byte_indices);
    }
    left.shuffle_bytes(right, indices)
}

/// Permutes four words, preserving any words outside the selected group.
pub(crate) fn shuffle_words<V: VectorType>(input: Val<V>, control: Val<I8>, high: bool) -> Val<V> {
    let control = control.unsigned().extend::<I16>();
    let first_lane = if high { 4 } else { 0 };
    let mut identity = V::Bytes::default();
    for (byte, index) in identity.as_mut().iter_mut().enumerate() {
        *index = byte as u8;
    }
    let mut indices: Val<V> = identity.into();
    for lane in 0_u32..4 {
        let selector = control.unsigned().shr(2 * lane).and(3);
        let source_byte_positions = if high { [8, 9] } else { [0, 1] };
        let byte_indices = selector
            .mul(u16::from_le_bytes([2; 2]) as u32)
            .add(u16::from_le_bytes(source_byte_positions) as u32);
        indices = indices.replace_lane(first_lane + lane, byte_indices);
    }
    input.shuffle_bytes(&input, indices)
}

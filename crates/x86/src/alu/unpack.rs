//! Packed unpacking interleaves equal-width lanes from the two input vectors.

use wasm86_compiler::{Val, V128};

/// Interleaves lanes starting at `first_byte` in each input.
/// MMX consumes the low qword of the result; XMM consumes all 128 bits.
pub(crate) fn unpack_lanes(
    left: Val<V128>,
    right: Val<V128>,
    lane_bytes: u8,
    first_byte: u8,
) -> Val<V128> {
    let lanes = std::array::from_fn(|byte| {
        let byte = byte as u8;
        let input = (byte / lane_bytes % 2) * 16;
        first_byte + input + (byte / (2 * lane_bytes)) * lane_bytes + byte % lane_bytes
    });
    left.shuffle_bytes(right, lanes)
}

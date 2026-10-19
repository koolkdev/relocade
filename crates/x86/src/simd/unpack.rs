//! Packed unpacking interleaves equal-width lanes from the two input vectors.

use relocade_compiler::{Val, VectorType};

/// Interleaves lanes starting at `first_byte` in each input.
/// The output fills the logical width of the input vectors.
pub(crate) fn unpack_lanes<V: VectorType>(
    left: Val<V>,
    right: Val<V>,
    lane_bytes: u8,
    first_byte: u8,
) -> Val<V> {
    let mut lanes = V::Bytes::default();
    let width = lanes.as_ref().len() as u8;
    for (byte, lane) in lanes.as_mut().iter_mut().enumerate() {
        let byte = byte as u8;
        let input = (byte / lane_bytes % 2) * width;
        *lane = first_byte + input + (byte / (2 * lane_bytes)) * lane_bytes + byte % lane_bytes;
    }
    left.shuffle_bytes(right, lanes)
}

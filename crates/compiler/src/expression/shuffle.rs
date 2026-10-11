//! Sixteen five-bit indices fit without enlarging scalar expression storage.

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) struct ShuffleMask([u8; 10]);

impl ShuffleMask {
    pub(crate) fn new(indices: [u8; 16]) -> Self {
        let mut bits = 0_u128;
        for (position, index) in indices.into_iter().enumerate() {
            assert!(index < 32, "shuffle byte is in range");
            bits |= u128::from(index) << (position * 5);
        }
        let bytes = bits.to_le_bytes();
        Self(std::array::from_fn(|index| bytes[index]))
    }

    pub(crate) fn indices(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[..10].copy_from_slice(&self.0);
        let bits = u128::from_le_bytes(bytes);
        std::array::from_fn(|position| ((bits >> (position * 5)) & 31) as u8)
    }
}

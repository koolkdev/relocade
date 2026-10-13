//! Explicit changes of vector width and access to a V64's raw scalar encoding.

use super::*;

impl<V: VectorType> Val<V> {
    pub(super) fn concat_low_halves(&self, high: Self) -> Val<V128> {
        self.binary(
            Binary::ShuffleImmediate(ShuffleMask::new(std::array::from_fn(|byte| {
                if byte < 8 {
                    byte as u8
                } else {
                    byte as u8 + 8
                }
            }))),
            high,
        )
    }
}

impl Val<V64> {
    /// Interprets all 64 integer bits as a vector. Upper carrier bits are unspecified.
    pub fn from_bits(bits: impl Into<Val<I64>>) -> Self {
        bits.into().unary(Unary::Reinterpret)
    }

    /// Returns the vector's complete 64-bit logical encoding.
    pub fn to_bits(&self) -> Val<I64> {
        self.unary(Unary::Reinterpret)
    }

    /// Widens to 128 bits, filling the upper half with zeros.
    pub fn zero_extend(&self) -> Val<V128> {
        self.unary(Unary::Convert)
    }

    /// Places this vector in the low half and `high` in the high half.
    pub fn concat(&self, high: impl Into<Self>) -> Val<V128> {
        self.concat_low_halves(high.into())
    }
}

impl Val<V128> {
    /// Keeps the low 64 logical bits. Upper carrier bits remain unobserved.
    pub fn low_half(&self) -> Val<V64> {
        self.unary(Unary::Convert)
    }
}

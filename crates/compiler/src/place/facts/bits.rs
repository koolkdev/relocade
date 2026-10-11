//! Partial knowledge of an encoding, independent of scalar or vector lane types.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct KnownBits {
    pub(super) mask: u128,
    pub(super) value: u128,
}

impl KnownBits {
    pub(super) fn with_zero_above(self, width: u8) -> Self {
        self.union(Self {
            mask: u128::MAX.checked_shl(u32::from(width)).unwrap_or(0),
            value: 0,
        })
    }
    pub(super) fn conflicts(self, other: Self) -> bool {
        (self.value ^ other.value) & self.mask & other.mask != 0
    }

    pub(super) fn union(self, other: Self) -> Self {
        Self {
            mask: self.mask | other.mask,
            value: self.value | other.value,
        }
    }

    pub(super) fn restrict(self, mask: u128) -> Self {
        Self {
            mask: self.mask & mask,
            value: self.value & mask,
        }
    }

    pub(super) fn common(self, other: Self) -> Self {
        let mask = self.mask & other.mask & !(self.value ^ other.value);
        Self {
            mask,
            value: self.value & mask,
        }
    }
}

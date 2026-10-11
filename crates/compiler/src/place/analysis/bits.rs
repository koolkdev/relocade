//! Known encoding positions, specialized to the storage width of their domain.

use crate::bitwise::BitwiseOp;
use std::ops::{BitAnd, BitOr, BitXor, Not};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Bits<T = u64> {
    pub(super) mask: T,
    pub(super) value: T,
}

impl<T> Bits<T>
where
    T: Copy
        + Default
        + Eq
        + BitAnd<Output = T>
        + BitOr<Output = T>
        + BitXor<Output = T>
        + Not<Output = T>,
{
    pub(super) fn conflicts(self, other: Self) -> bool {
        (self.value ^ other.value) & self.mask & other.mask != T::default()
    }

    pub(super) fn union(self, other: Self) -> Self {
        Self {
            mask: self.mask | other.mask,
            value: self.value | other.value,
        }
    }

    pub(super) fn restrict(self, mask: T) -> Self {
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

    pub(super) fn bitwise(self, operator: BitwiseOp, other: Self) -> Self {
        match operator {
            BitwiseOp::And => {
                let zeros = (self.mask & !self.value) | (other.mask & !other.value);
                let ones = self.value & other.value;
                Self {
                    mask: zeros | ones,
                    value: ones,
                }
            }
            BitwiseOp::Or => {
                let ones = self.value | other.value;
                Self {
                    mask: (self.mask & !self.value & other.mask & !other.value) | ones,
                    value: ones,
                }
            }
            BitwiseOp::Xor => {
                let mask = self.mask & other.mask;
                Self {
                    mask,
                    value: (self.value ^ other.value) & mask,
                }
            }
        }
    }

    /// Facts implied about each input by an observed bitwise result.
    pub(super) fn input_constraints(
        self,
        operator: BitwiseOp,
        left: Self,
        right: Self,
    ) -> [Self; 2] {
        match operator {
            BitwiseOp::Or => {
                [Self {
                    mask: self.mask & !self.value,
                    value: T::default(),
                }; 2]
            }
            BitwiseOp::And => {
                let ones = Self {
                    mask: self.value,
                    value: self.value,
                };
                [
                    ones.union(self.restrict(right.value)),
                    ones.union(self.restrict(left.value)),
                ]
            }
            BitwiseOp::Xor => [Self::default(); 2],
        }
    }

    /// Whether this operand supplies every observed output position unchanged.
    pub(super) fn absorbs(self, operator: BitwiseOp, other: Self, observed: T) -> bool {
        let zeros = self.mask & !self.value;
        let other_zeros = other.mask & !other.value;
        match operator {
            BitwiseOp::Or => observed & !other_zeros & !self.value == T::default(),
            BitwiseOp::And => observed & !other.value & !zeros == T::default(),
            BitwiseOp::Xor => false,
        }
    }
}

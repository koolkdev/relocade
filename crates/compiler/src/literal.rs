//! Literal bits; the owning value supplies their logical type.

use crate::Type;

/// Two words retain 128 bits without imposing u128 alignment on the IR.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Literal {
    low: u64,
    high: u64,
}

impl Literal {
    pub(crate) fn normalize(self, ty: Type) -> Self {
        if ty.is_scalar() {
            Self::from(ty.normalize(u64::from(self)))
        } else {
            self
        }
    }

    pub(crate) fn scalar(self, ty: Type) -> Option<u64> {
        ty.is_scalar().then(|| u64::from(self))
    }
}

impl From<u64> for Literal {
    fn from(bits: u64) -> Self {
        Self { low: bits, high: 0 }
    }
}

impl From<i32> for Literal {
    fn from(bits: i32) -> Self {
        Self::from(bits as i64 as u64)
    }
}

impl From<u128> for Literal {
    fn from(bits: u128) -> Self {
        Self {
            low: bits as u64,
            high: (bits >> 64) as u64,
        }
    }
}

impl From<Literal> for u64 {
    fn from(literal: Literal) -> Self {
        debug_assert_eq!(literal.high, 0, "scalar literal bits fit in one word");
        literal.low
    }
}

impl From<Literal> for u128 {
    fn from(literal: Literal) -> Self {
        u128::from(literal.low) | (u128::from(literal.high) << 64)
    }
}

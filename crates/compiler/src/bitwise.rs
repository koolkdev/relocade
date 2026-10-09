//! Bitwise operations on value encodings.

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum BitwiseOp {
    And,
    Or,
    Xor,
}

impl BitwiseOp {
    pub(crate) fn apply(self, left: u64, right: u64) -> u64 {
        match self {
            Self::And => left & right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
        }
    }
}

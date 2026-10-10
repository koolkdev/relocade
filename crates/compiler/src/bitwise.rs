//! Bitwise operations on complete literal encodings.

use crate::literal::Literal;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum BitwiseOp {
    And,
    Or,
    Xor,
}

impl BitwiseOp {
    pub(crate) fn apply(self, left: Literal, right: Literal) -> Literal {
        let left = u128::from(left);
        let right = u128::from(right);
        match self {
            Self::And => left & right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
        }
        .into()
    }
}

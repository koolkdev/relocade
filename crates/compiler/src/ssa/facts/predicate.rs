//! Canonical Boolean propositions refer to runtime values, including refinements.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Predicate {
    Equal(usize, usize),
    UnsignedLess(usize, usize),
    SignedLess(usize, usize),
    Zero(usize),
}

impl Predicate {
    pub(crate) fn inputs(self) -> [usize; 2] {
        match self {
            Self::Equal(a, b) | Self::UnsignedLess(a, b) | Self::SignedLess(a, b) => [a, b],
            Self::Zero(a) => [a, a],
        }
    }
}

pub(in crate::ssa) fn identify(
    values: &ValueTable,
    expression: Expression<usize>,
    identity: impl Fn(usize) -> usize,
) -> Option<(Predicate, bool)> {
    match expression {
        Expression::Unary {
            operator: Unary::ZeroTest { nonzero },
            input,
        } => Some((Predicate::Zero(identity(input)), !nonzero)),
        Expression::Binary {
            operator: Binary::Compare(Compare::Integer(operator)),
            left,
            right,
        } => {
            let a = identity(left);
            let b = identity(right);
            Some(match operator {
                CompareOp::Eq | CompareOp::Ne => {
                    let key = if values[left].scalar_literal() == Some(0) {
                        Predicate::Zero(b)
                    } else if values[right].scalar_literal() == Some(0) {
                        Predicate::Zero(a)
                    } else {
                        Predicate::Equal(a.min(b), a.max(b))
                    };
                    (key, operator == CompareOp::Eq)
                }
                CompareOp::LtUnsigned | CompareOp::GeUnsigned => (
                    Predicate::UnsignedLess(a, b),
                    operator == CompareOp::LtUnsigned,
                ),
                CompareOp::LtSigned | CompareOp::GeSigned => {
                    (Predicate::SignedLess(a, b), operator == CompareOp::LtSigned)
                }
            })
        }
        _ => None,
    }
}

//! Physical known bits attached to explicit definitions.
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    expression::{
        BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, TernaryOp as Ternary,
        UnaryOp as Unary,
    },
    integer::{self, CompareOp, ShiftOp},
    Expression, Type,
};
mod bits;
mod predicate;
mod range;
mod rounding;
pub(super) use bits::Bits;
use bits::{infer_bits, initial_bits};
pub(super) use predicate::{identify as predicate, Predicate};
use range::Range;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct Summary {
    pub(super) bits: Bits,
    range: Range,
}

impl From<Bits> for Summary {
    fn from(bits: Bits) -> Self {
        Self {
            bits,
            range: Range::default(),
        }
    }
}

impl Summary {
    pub(super) fn known(self, mask: u64) -> Option<u64> {
        self.bits.known(mask)
    }
    pub(super) fn restrict(self, mask: u64) -> Self {
        let range = self.range.intersect(Range::from_bits(self.bits, mask));
        Self {
            bits: self.bits.restrict(mask).union(range.bits(mask)),
            range,
        }
    }
    pub(super) fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits.union(other.bits),
            range: self.range.intersect(other.range),
        }
    }
    pub(super) fn intersect(self, other: Self) -> Self {
        Self {
            bits: self.bits.intersect(other.bits),
            range: self.range.hull(other.range),
        }
    }
    pub(super) fn conflicts(self, other: Self) -> bool {
        self.bits.conflicts(other.bits)
            || self.range.min > other.range.max
            || other.range.min > self.range.max
    }
}

pub(super) fn infer(
    values: &ValueTable<impl crate::body::Phase>,
    id: usize,
    summaries: &[Summary],
) -> Summary {
    infer_with(values, id, |input| {
        summaries.get(input).copied().unwrap_or_else(|| {
            let bits = initial_bits(values, input);
            if matches!(
                values[input].ty,
                Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64
            ) {
                Summary {
                    bits,
                    range: Range::from_bits(bits, values[input].ty.carrier().mask()),
                }
            } else {
                bits.into()
            }
        })
    })
}

pub(super) fn infer_with(
    values: &ValueTable<impl crate::body::Phase>,
    id: usize,
    get: impl Fn(usize) -> Summary,
) -> Summary {
    let mut bits = infer_bits(values, id, &get);
    if !matches!(
        values[id].ty,
        Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64
    ) {
        return bits.into();
    }
    if let ValueDefinition::Expression(Expression::Binary {
        operator: Binary::Compare(Compare::Integer(operator)),
        left,
        right,
    }) = values[id].definition
    {
        if let Some(truth) = range::comparison(operator, get(left).range, get(right).range) {
            bits = Bits::constant(u64::from(truth), values[id].ty.carrier().mask());
        }
    }
    if let ValueDefinition::Expression(Expression::Unary {
        operator: Unary::ZeroTest { nonzero },
        input,
    }) = values[id].definition
    {
        let range = get(input).range;
        if range.min > 0 || range.max == 0 {
            bits = Bits::constant(
                u64::from((range.min > 0) == nonzero),
                values[id].ty.carrier().mask(),
            );
        }
    }
    let range = range::infer(values, id, &get, bits);
    Summary {
        bits: bits.union(range.bits(values[id].ty.carrier().mask())),
        range,
    }
}

pub(super) fn backward_ranges(
    values: &ValueTable<impl crate::body::Phase>,
    ty: Type,
    expression: Expression<usize>,
    observed: Summary,
    get: impl Fn(usize) -> Summary,
) -> Vec<(usize, Summary)> {
    range::backward(values, ty, expression, observed, get)
}

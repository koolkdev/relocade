//! Unsigned physical intervals; operations which wrap keep conservative bounds.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Range {
    pub(super) min: u64,
    pub(super) max: u64,
}

impl Default for Range {
    fn default() -> Self {
        Self {
            min: 0,
            max: u64::MAX,
        }
    }
}

impl Range {
    pub(super) fn from_bits(bits: Bits, mask: u64) -> Self {
        Self {
            min: bits.one & mask,
            max: !bits.zero & mask,
        }
    }
    pub(super) fn bits(self, mask: u64) -> Bits {
        let differing = self.min ^ self.max;
        let fixed = mask & !integer::low_mask((64 - differing.leading_zeros()) as u8);
        Bits::constant(self.min, fixed)
    }
    pub(super) fn intersect(self, other: Self) -> Self {
        Self {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
    }
    pub(super) fn hull(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

pub(super) fn infer(
    values: &ValueTable,
    id: usize,
    summaries: impl Fn(usize) -> Summary,
    bits: Bits,
) -> Range {
    let mask = values[id].ty.carrier().mask();
    let base = Range::from_bits(bits, mask);
    let get = |id: usize| summaries(id).range;
    let ValueDefinition::Expression(expression) = values[id].definition else {
        return base;
    };
    let range = match expression {
        Expression::Unary {
            operator: Unary::Convert,
            input,
        } if get(input).max <= mask => get(input),
        Expression::Unary {
            operator: Unary::LowBits { bits },
            input,
        } if get(input).max <= integer::low_mask(bits) => get(input),
        Expression::Ternary {
            operator: Ternary::Select,
            first,
            second,
            ..
        } => get(first).hull(get(second)),
        Expression::Binary {
            operator: Binary::Integer(operator),
            left,
            right,
        } => {
            let a = get(left);
            let b = get(right);
            match operator {
                integer::BinaryOp::Add
                    if u128::from(a.max) + u128::from(b.max) <= u128::from(mask) =>
                {
                    Range {
                        min: a.min + b.min,
                        max: a.max + b.max,
                    }
                }
                integer::BinaryOp::Sub if a.min >= b.max => Range {
                    min: a.min - b.max,
                    max: a.max - b.min,
                },
                integer::BinaryOp::Mul
                    if u128::from(a.max) * u128::from(b.max) <= u128::from(mask) =>
                {
                    Range {
                        min: a.min * b.min,
                        max: a.max * b.max,
                    }
                }
                _ => base,
            }
        }
        _ => base,
    };
    base.intersect(range)
}

pub(super) fn comparison(operator: CompareOp, a: Range, b: Range) -> Option<bool> {
    match operator {
        CompareOp::LtUnsigned | CompareOp::GeUnsigned if a.max < b.min => {
            Some(operator == CompareOp::LtUnsigned)
        }
        CompareOp::LtUnsigned | CompareOp::GeUnsigned if a.min >= b.max => {
            Some(operator == CompareOp::GeUnsigned)
        }
        CompareOp::Eq | CompareOp::Ne if a.max < b.min || b.max < a.min => {
            Some(operator == CompareOp::Ne)
        }
        _ => None,
    }
}

pub(super) fn backward(
    values: &ValueTable,
    ty: Type,
    expression: Expression<usize>,
    observed: Summary,
    get: impl Fn(usize) -> Summary,
) -> Vec<(usize, Summary)> {
    let mut result = Vec::new();
    let mut refine = |id, range: Range| {
        if range.min <= range.max {
            result.push((
                id,
                Summary {
                    bits: Bits::default(),
                    range,
                },
            ));
        }
    };
    match expression {
        Expression::Unary {
            operator: Unary::Convert,
            input,
        } if values[input].ty.carrier().bits() <= ty.carrier().bits() => {
            refine(input, observed.range)
        }
        Expression::Unary {
            operator: Unary::ZeroTest { nonzero },
            input,
        } => {
            if let Some(truth) = observed.bits.known(1) {
                refine(
                    input,
                    if (truth != 0) == nonzero {
                        Range {
                            min: 1,
                            max: values[input].ty.carrier().mask(),
                        }
                    } else {
                        Range { min: 0, max: 0 }
                    },
                );
            }
        }
        Expression::Binary {
            operator: Binary::Compare(Compare::Integer(operator)),
            left,
            right,
        } => {
            let Some(truth) = observed.bits.known(1) else {
                return result;
            };
            let a = get(left).range;
            let b = get(right).range;
            if matches!(operator, CompareOp::Eq | CompareOp::Ne) {
                if (truth != 0) == (operator == CompareOp::Eq) {
                    refine(left, b);
                    refine(right, a);
                } else {
                    for (id, own, other) in [(left, a, b), (right, b, a)] {
                        if other.min == other.max {
                            if own.min == other.min {
                                if let Some(min) = own.min.checked_add(1) {
                                    refine(id, Range { min, max: own.max });
                                }
                            } else if own.max == other.min {
                                if let Some(max) = own.max.checked_sub(1) {
                                    refine(id, Range { min: own.min, max });
                                }
                            }
                        }
                    }
                }
            } else if matches!(operator, CompareOp::LtUnsigned | CompareOp::GeUnsigned) {
                if (truth != 0) == (operator == CompareOp::LtUnsigned) {
                    if let Some(max) = b.max.checked_sub(1) {
                        refine(left, Range { min: 0, max });
                    }
                    if let Some(min) = a.min.checked_add(1) {
                        refine(right, Range { min, max: u64::MAX });
                    }
                } else {
                    refine(
                        left,
                        Range {
                            min: b.min,
                            max: u64::MAX,
                        },
                    );
                    refine(right, Range { min: 0, max: a.max });
                }
            }
        }
        _ => {}
    }
    result
}

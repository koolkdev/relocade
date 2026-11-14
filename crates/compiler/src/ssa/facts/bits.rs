//! Known physical bits and direct operation transfers.
use super::*;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(in crate::ssa) struct Bits {
    pub(in crate::ssa) zero: u64,
    pub(in crate::ssa) one: u64,
}

impl Bits {
    pub(in crate::ssa) fn constant(value: u64, mask: u64) -> Self {
        Self {
            zero: !value & mask,
            one: value & mask,
        }
    }
    pub(in crate::ssa) fn known(self, mask: u64) -> Option<u64> {
        ((self.zero | self.one) & mask == mask).then_some(self.one & mask)
    }
    pub(in crate::ssa) fn intersect(self, other: Self) -> Self {
        Self {
            zero: self.zero & other.zero,
            one: self.one & other.one,
        }
    }
    pub(in crate::ssa) fn restrict(self, mask: u64) -> Self {
        Self {
            zero: self.zero & mask,
            one: self.one & mask,
        }
    }
    pub(in crate::ssa) fn union(self, other: Self) -> Self {
        Self {
            zero: self.zero | other.zero,
            one: self.one | other.one,
        }
    }
    pub(in crate::ssa) fn conflicts(self, other: Self) -> bool {
        (self.zero & other.one) | (self.one & other.zero) != 0
    }
}

pub(super) fn initial_bits(values: &ValueTable, id: usize) -> Bits {
    let value = values[id];
    if !matches!(
        value.ty,
        Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64
    ) {
        return Bits::default();
    }
    let mask = value.ty.carrier().mask();
    if let Some(bits) = value.scalar_literal() {
        return Bits::constant(bits, mask);
    }
    Bits {
        zero: mask & !integer::low_mask(values.bounds[id].unsigned),
        one: 0,
    }
}

pub(super) fn infer_bits(values: &ValueTable, id: usize, get: impl Fn(usize) -> Summary) -> Bits {
    let base = initial_bits(values, id);
    let ty = values[id].ty;
    if !matches!(ty, Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64) {
        return base;
    }
    let mask = ty.carrier().mask();
    let bits = |id: usize| get(id).bits;
    let ValueDefinition::Expression(expression) = values[id].definition else {
        return base;
    };
    let inferred = match expression {
        Expression::Unary { operator, input } => {
            let a = bits(input);
            match operator {
                Unary::Convert => a,
                Unary::LowBits { bits } => a.restrict(integer::low_mask(bits)).union(Bits {
                    zero: mask & !integer::low_mask(bits),
                    one: 0,
                }),
                Unary::ZeroTest { nonzero } => {
                    let source_mask = values[input].ty.carrier().mask();
                    if a.one != 0 {
                        Bits::constant(u64::from(nonzero), mask)
                    } else if a.zero & source_mask == source_mask {
                        Bits::constant(u64::from(!nonzero), mask)
                    } else {
                        base
                    }
                }
                Unary::SignExtend { input_type } => {
                    let low = integer::low_mask(input_type.bits());
                    let sign = 1 << (input_type.bits() - 1);
                    a.restrict(low).union(Bits {
                        zero: if a.zero & sign != 0 { mask & !low } else { 0 },
                        one: if a.one & sign != 0 { mask & !low } else { 0 },
                    })
                }
                _ => base,
            }
        }
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let a = bits(left);
            let b = bits(right);
            match operator {
                Binary::Bitwise(BitwiseOp::And) => Bits {
                    zero: a.zero | b.zero,
                    one: a.one & b.one,
                },
                Binary::Bitwise(BitwiseOp::Or) => Bits {
                    zero: a.zero & b.zero,
                    one: a.one | b.one,
                },
                Binary::Bitwise(BitwiseOp::Xor) => Bits {
                    zero: (a.zero & b.zero) | (a.one & b.one),
                    one: (a.zero & b.one) | (a.one & b.zero),
                },
                Binary::Compare(Compare::Integer(operator)) => compare(
                    operator,
                    a,
                    b,
                    values[left].ty.carrier().mask(),
                    left == right,
                )
                .map(|truth| Bits::constant(u64::from(truth), mask))
                .unwrap_or(base),
                Binary::Integer(operator @ (integer::BinaryOp::Add | integer::BinaryOp::Sub)) => {
                    let common = (a.zero | a.one) & (b.zero | b.one);
                    let low = integer::low_mask(common.trailing_ones() as u8);
                    let value = if operator == integer::BinaryOp::Add {
                        a.one.wrapping_add(b.one)
                    } else {
                        a.one.wrapping_sub(b.one)
                    };
                    Bits::constant(value, low)
                }
                _ => base,
            }
        }
        Expression::Shift {
            operator: Shift::Scalar(operator),
            value,
            count,
        } => {
            let a = bits(value);
            let count_mask = if ty == Type::I64 { 63 } else { 31 };
            if let Some(count) = bits(count).known(count_mask) {
                let count = count as u32;
                match operator {
                    ShiftOp::Left => Bits {
                        zero: (a.zero << count) | integer::low_mask(count as u8),
                        one: a.one << count,
                    },
                    ShiftOp::RightUnsigned => Bits {
                        zero: (a.zero >> count) | (mask & !(mask >> count)),
                        one: a.one >> count,
                    },
                    ShiftOp::RightSigned => {
                        let high = mask & !(mask >> count);
                        let sign = 1 << (ty.carrier().bits() - 1);
                        Bits {
                            zero: (a.zero >> count) | if a.zero & sign != 0 { high } else { 0 },
                            one: (a.one >> count) | if a.one & sign != 0 { high } else { 0 },
                        }
                    }
                }
            } else {
                base
            }
        }
        Expression::Ternary {
            operator: Ternary::Select,
            first,
            second,
            third,
        } => match bits(third).known(1) {
            Some(0) => bits(second),
            Some(_) => bits(first),
            None => bits(first).intersect(bits(second)),
        },
        _ => base,
    };
    base.union(inferred.restrict(mask))
}

fn compare(operator: CompareOp, a: Bits, b: Bits, mask: u64, same: bool) -> Option<bool> {
    if same {
        return Some(matches!(
            operator,
            CompareOp::Eq | CompareOp::GeUnsigned | CompareOp::GeSigned
        ));
    }
    match operator {
        CompareOp::Eq | CompareOp::Ne if a.conflicts(b) => Some(operator == CompareOp::Ne),
        CompareOp::LtUnsigned | CompareOp::GeUnsigned => {
            if mask & !a.zero < b.one {
                Some(operator == CompareOp::LtUnsigned)
            } else if a.one >= mask & !b.zero {
                Some(operator == CompareOp::GeUnsigned)
            } else {
                None
            }
        }
        _ => None,
    }
}

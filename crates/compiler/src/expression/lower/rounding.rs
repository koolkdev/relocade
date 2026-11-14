use super::{fold, unary, Binary, BitwiseOp, Expression, Ternary, Type, Unary, ValueTable};
use crate::{
    body::{Definitions, ValueDefinition},
    expression::{
        rounding::{AWAY, GUARD, NEAREST, STICKY},
        CompareOp, ShiftOp,
    },
    integer,
};

pub(super) fn lower(
    values: &mut ValueTable<Definitions>,
    producer: usize,
    integer: usize,
    distance: usize,
    control: usize,
    demanded: &[bool],
) -> Vec<(usize, usize)> {
    let mut expansion = Expansion { values };
    let e = &mut expansion;
    let need_integer = demanded[producer];
    let need_increment = need_integer || demanded[producer + 2];
    let nearest = e.flag(control, NEAREST);
    let away = e.flag(control, AWAY);
    let need_nearest = need_increment && !e.is_false(nearest);
    let need_inexact = demanded[producer + 1] || (need_increment && !e.is_false(away));
    let zero = e.values.literal(Type::I1, 0);
    let mut floor = e.values.literal(Type::I64, 0);
    if need_integer || need_nearest {
        let below_word = e.less_than(distance, 64);
        let shifted = e.shift(integer, distance, integer::ShiftOp::RightUnsigned);
        floor = e.select(below_word, shifted, floor);
    }
    let mut inexact = zero;
    let mut nearest_increment = zero;
    if need_nearest {
        let original_guard = e.flag(control, GUARD);
        let original_sticky = e.flag(control, STICKY);
        let shifted = e.nonzero(distance);
        let one = e.values.literal(Type::I32, 1);
        let preceding = e.binary(Binary::Integer(integer::BinaryOp::Sub), distance, one);
        let first_discarded = e.shift(integer, preceding, integer::ShiftOp::RightUnsigned);
        let first_discarded = e.flag(first_discarded, 1);
        let within_word = e.less_than(distance, 65);
        let guard = e.binary(
            Binary::Bitwise(BitwiseOp::And),
            within_word,
            first_discarded,
        );
        let guard = e.select(shifted, guard, original_guard);
        let lower = e.discarded(integer, preceding);
        let lower = e.binary(Binary::Bitwise(BitwiseOp::Or), original_guard, lower);
        let lower = e.binary(Binary::Bitwise(BitwiseOp::And), shifted, lower);
        let sticky = e.binary(Binary::Bitwise(BitwiseOp::Or), original_sticky, lower);
        if need_inexact {
            inexact = e.binary(Binary::Bitwise(BitwiseOp::Or), guard, sticky);
        }
        let odd = e.flag(floor, 1);
        let above_tie = e.binary(Binary::Bitwise(BitwiseOp::Or), sticky, odd);
        nearest_increment = e.binary(Binary::Bitwise(BitwiseOp::And), guard, above_tie);
    } else if need_inexact {
        let original_guard = e.flag(control, GUARD);
        let original_sticky = e.flag(control, STICKY);
        let fraction = e.binary(
            Binary::Bitwise(BitwiseOp::Or),
            original_guard,
            original_sticky,
        );
        let discarded = e.discarded(integer, distance);
        inexact = e.binary(Binary::Bitwise(BitwiseOp::Or), fraction, discarded);
    }
    let mut incremented = zero;
    if need_increment {
        let directed = e.binary(Binary::Bitwise(BitwiseOp::And), away, inexact);
        incremented = e.select(nearest, nearest_increment, directed);
    }
    let mut results = Vec::with_capacity(3);
    if need_integer {
        let increment = unary(e.values, Type::I64, Unary::Convert, incremented);
        let rounded = e.binary(Binary::Integer(integer::BinaryOp::Add), floor, increment);
        results.push((producer, rounded));
    }
    if demanded[producer + 1] {
        results.push((producer + 1, inexact));
    }
    if demanded[producer + 2] {
        results.push((producer + 2, incremented));
    }
    results
}

struct Expansion<'a> {
    values: &'a mut ValueTable<Definitions>,
}

impl Expansion<'_> {
    fn is_false(&self, value: usize) -> bool {
        self.values[value].scalar_literal() == Some(0)
    }

    fn binary(&mut self, operator: Binary, left: usize, right: usize) -> usize {
        let ty = if matches!(operator, Binary::Compare(_)) {
            Type::I1
        } else {
            self.values[left].ty
        };
        fold(
            self.values,
            ty,
            Expression::Binary {
                operator,
                left,
                right,
            },
        )
    }

    fn select(&mut self, condition: usize, yes: usize, no: usize) -> usize {
        fold(
            self.values,
            self.values[yes].ty,
            Expression::Ternary {
                operator: Ternary::Select,
                first: yes,
                second: no,
                third: condition,
            },
        )
    }

    fn shift(&mut self, value: usize, count: usize, operator: integer::ShiftOp) -> usize {
        let count = unary(
            self.values,
            self.values[value].ty.carrier(),
            Unary::Convert,
            count,
        );
        fold(
            self.values,
            self.values[value].ty,
            Expression::Shift {
                operator: ShiftOp::Scalar(operator),
                value,
                count,
            },
        )
    }

    fn less_than(&mut self, value: usize, limit: u64) -> usize {
        let limit = self.values.literal(self.values[value].ty, limit);
        self.binary(
            Binary::Compare(CompareOp::Integer(integer::CompareOp::LtUnsigned)),
            value,
            limit,
        )
    }

    fn nonzero(&mut self, value: usize) -> usize {
        unary(
            self.values,
            Type::I1,
            Unary::ZeroTest { nonzero: true },
            value,
        )
    }

    /// Whether the shift discards any integer bit, including counts of 64 or more.
    fn discarded(&mut self, integer: usize, distance: usize) -> usize {
        let one = self.values.literal(Type::I64, 1);
        let top = self.shift(one, distance, integer::ShiftOp::Left);
        let mask = self.binary(Binary::Integer(integer::BinaryOp::Sub), top, one);
        let low = self.binary(Binary::Bitwise(BitwiseOp::And), integer, mask);
        let low = self.nonzero(low);
        let any = self.nonzero(integer);
        let below_word = self.less_than(distance, 64);
        self.select(below_word, low, any)
    }

    fn flag(&mut self, value: usize, mask: u8) -> usize {
        self.flag_at(value, u64::from(mask), 8)
    }

    fn flag_at(&mut self, value: usize, mask: u64, remaining: u8) -> usize {
        let value = self.values.representation(value);
        if let Some(bits) = self.values[value].scalar_literal() {
            return self.values.literal(Type::I1, u64::from(bits & mask != 0));
        }
        if mask == 0 || self.values.bounds[value].unsigned <= mask.trailing_zeros() as u8 {
            return self.values.literal(Type::I1, 0);
        }
        if mask == 1 && self.values.bounds[value].unsigned <= 1 {
            return unary(self.values, Type::I1, Unary::Convert, value);
        }
        // Unpack the short control expression built by RoundingInput. Stop at
        // its boolean leaves instead of carrying their packing into Wasm.
        if remaining != 0 {
            match self.values[value].definition {
                ValueDefinition::Expression(Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::Or),
                    left,
                    right,
                }) => {
                    let left = self.flag_at(left, mask, remaining - 1);
                    let right = self.flag_at(right, mask, remaining - 1);
                    return self.binary(Binary::Bitwise(BitwiseOp::Or), left, right);
                }
                ValueDefinition::Expression(Expression::Shift {
                    operator: ShiftOp::Scalar(integer::ShiftOp::Left),
                    value: input,
                    count,
                }) => {
                    if let Some(count) = self.values[count].scalar_literal() {
                        let count = integer::shift_count(self.values[value].ty, count as u32);
                        return self.flag_at(input, mask >> count, remaining - 1);
                    }
                }
                _ => {}
            }
        }
        let mask = self.values.literal(self.values[value].ty, mask);
        let bit = self.binary(Binary::Bitwise(BitwiseOp::And), value, mask);
        self.nonzero(bit)
    }
}

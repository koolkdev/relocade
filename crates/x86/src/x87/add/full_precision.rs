//! Full-precision sums of normal operands and signed zeros.

use relocade_compiler::{BlockBuilder, BuildError, Results, Val, I1, I16, I32, I64};

use crate::floating::{FiniteInput, FiniteMagnitude, RoundingInput, RoundingMode};
use crate::x87::{
    operand::{AddendSigns, BinaryOperands},
    result::RoundedValue,
    value::KnownClass,
    ArithmeticCandidate, BinaryOperation, ExtendedBits, ExtendedValue,
};

const LEADING: u64 = 1 << 63;

// Keep the exponent unbounded across the join so range admission can test it.
type SumShape = (I64, I32, I1, I1, I1);
type Components = <SumShape as Results>::Values;

pub(in crate::x87) fn full_precision_candidate(
    body: &mut BlockBuilder<'_>,
    operands: &BinaryOperands,
    operation: BinaryOperation,
    rounding: &RoundingMode,
) -> Result<ArithmeticCandidate, BuildError> {
    let addends = Addends {
        left: &operands.left.bits,
        right: &operands.right.bits,
        signs: operands.addend_signs(operation),
        rounding,
    };
    let gap = addends
        .left
        .exponent_field()
        .sub(addends.right.exponent_field());
    // Outside [-65, 65] the smaller magnitude is below a quarter ulp of the
    // larger binade, hence below half an ulp even across its lower boundary.
    let dominant = gap.add(65).unsigned().ge(131);
    let nearest = body.constant_bits(rounding.is_nearest())? == Some(1);
    let (significand, exponent, negative, inexact, incremented) = body.if_value::<SumShape>(
        dominant,
        |body| body.yield_(addends.dominant(nearest)),
        |mut body| {
            let sum = addends.nearby_exponents(&mut body, gap.eq(0))?;
            body.yield_(sum)
        },
    )?;
    let zero = significand.eq(0_u64);
    // At PC64 a tiny cancellation is exact on the binary80 subnormal grid.
    let in_range = zero.or(exponent.sub(1).unsigned().lt(0x7ffe));
    let value = ExtendedValue::from_bits(ExtendedBits {
        significand,
        sign_exponent: zero
            .select(0, exponent)
            .or(negative.select(0x8000, 0))
            .truncate::<I16>(),
    })
    .assume_class(KnownClass::zero().select(&zero, &KnownClass::normal()));
    Ok(ArithmeticCandidate {
        valid: operands.normal_or_zero().and(in_range),
        rounded: RoundedValue {
            value,
            inexact,
            incremented,
        },
    })
}

struct Addends<'a> {
    left: &'a ExtendedBits,
    right: &'a ExtendedBits,
    signs: AddendSigns,
    rounding: &'a RoundingMode,
}

impl Addends<'_> {
    fn subtract_magnitudes(&self) -> Val<I1> {
        self.signs.left_negative.xor(&self.signs.right_negative)
    }

    /// At unequal exponents, the larger operand determines the result sign.
    fn ordered(&self) -> OrderedAddends {
        let left = self.left.exponent_field();
        let right = self.right.exponent_field();
        let left_larger = right.unsigned().lt(&left);
        let exponent = left_larger.select(&left, &right);
        OrderedAddends {
            leading: FiniteInput {
                significand: left_larger.select(&self.left.significand, &self.right.significand),
                exponent: exponent.sub(16383),
                negative: left_larger.select(&self.signs.left_negative, &self.signs.right_negative),
            },
            trailing: left_larger.select(&self.right.significand, &self.left.significand),
            gap: exponent.sub(left_larger.select(&right, &left)),
        }
    }

    fn dominant(&self, nearest: bool) -> Components {
        let OrderedAddends {
            leading, trailing, ..
        } = self.ordered();
        let inexact = trailing.ne(0_u64);
        let borrow = self.subtract_magnitudes().and(&inexact);
        if nearest {
            // Rounding restores the larger operand. After magnitude subtraction
            // that restoration increases the magnitude above its truncation.
            return (
                leading.significand,
                leading.exponent.add(16383),
                leading.negative,
                inexact,
                borrow,
            );
        }
        let mut magnitude = leading.magnitude().with_negative_tail(&borrow);
        magnitude.significand.sticky = inexact;
        round_nonzero(magnitude, self.rounding)
    }

    fn nearby_exponents(
        &self,
        body: &mut BlockBuilder<'_>,
        equal: Val<I1>,
    ) -> Result<Components, BuildError> {
        body.if_value::<SumShape>(
            equal,
            |mut body| {
                let sum = self.equal_exponents(&mut body)?;
                body.yield_(sum)
            },
            |mut body| {
                let sum = self.overlapping(&mut body)?;
                body.yield_(sum)
            },
        )
    }

    fn equal_exponents(&self, body: &mut BlockBuilder<'_>) -> Result<Components, BuildError> {
        let left = &self.left.significand;
        let right = &self.right.significand;
        let exponent = self.left.exponent_field();
        body.if_value::<SumShape>(
            self.subtract_magnitudes(),
            |body| {
                // The difference is exact, even after deep cancellation.
                let left_larger = left.unsigned().ge(right);
                let difference = left_larger.select(left.sub(right), right.sub(left));
                let zero = difference.eq(0_u64);
                let negative = zero.select(
                    self.rounding.cancellation_negative(),
                    left_larger.select(&self.signs.left_negative, &self.signs.right_negative),
                );
                let shift = difference.clz().truncate::<I32>();
                body.yield_((
                    difference.shl(&shift),
                    exponent.sub(shift),
                    negative,
                    false,
                    false,
                ))
            },
            |body| {
                // Two nonzero significands always carry. Only the low bit is lost.
                let sum = left.add(right);
                let rounded = self.rounding.round(
                    RoundingInput {
                        integer: sum.unsigned().shr(1).or(LEADING),
                        guard: sum.and(1_u64).ne(0_u64),
                        sticky: false.into(),
                    },
                    &self.signs.left_negative,
                );
                body.yield_((
                    exponent.eq(0).select(0_u64, rounded.integer),
                    exponent.add(1),
                    &self.signs.left_negative,
                    rounded.inexact,
                    rounded.incremented,
                ))
            },
        )
    }

    fn overlapping(&self, body: &mut BlockBuilder<'_>) -> Result<Components, BuildError> {
        let OrderedAddends {
            leading,
            trailing,
            gap,
        } = self.ordered();
        let magnitude = leading.add_smaller(body, &trailing, &gap, &self.subtract_magnitudes())?;
        Ok(round_nonzero(magnitude, self.rounding))
    }
}

struct OrderedAddends {
    leading: FiniteInput,
    trailing: Val<I64>,
    gap: Val<I32>,
}

fn round_nonzero(value: FiniteMagnitude, rounding: &RoundingMode) -> Components {
    let rounded = rounding.round(value.significand, &value.negative);
    let carry = rounded.integer.eq(0_u64);
    (
        carry.select(LEADING, rounded.integer),
        value
            .exponent
            .add(16383)
            .add(carry.unsigned().extend::<I32>()),
        value.negative,
        rounded.inexact,
        rounded.incremented,
    )
}

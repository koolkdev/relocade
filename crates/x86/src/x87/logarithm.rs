//! Logarithms keep cancellation and extended exponents inside their numerical owner.

use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32};

use crate::floating::RoundingMode;

use super::{
    result::RoundedValue,
    transcendental::{odd_power_series, DoubleDouble, DoubleShape},
    ArithmeticResult, BinaryOperands, ExtendedValue, Operand,
};

#[derive(Clone, Copy)]
pub(crate) enum LogarithmArgument {
    Value,
    Increment,
}

/// A factored logarithm retains its exponent until multiplication by y.
/// Zero uses a nonzero placeholder coefficient, replaced after rounding.
struct Logarithm {
    coefficient: DoubleDouble,
    exponent: Val<I32>,
    negative: Val<I1>,
    zero: Val<I1>,
    discarded_tail: Val<I1>,
}

impl Logarithm {
    fn multiply(&self, y: &Operand, rounding: &RoundingMode) -> ArithmeticResult {
        let (significand, exponent) = y.normalized();
        let multiplier = DoubleDouble::significand(&y.zero.select(1_u64 << 63, significand));
        let negative = y.bits.negative().xor(&self.negative);
        let mut result = multiplier
            .mul(&self.coefficient)
            .magnitude(
                exponent.add(&self.exponent),
                negative.clone(),
                self.discarded_tail.clone(),
            )
            .round(3.into(), rounding);
        // PE also applies when an integral logarithm gives a representable
        // nonzero product. This evidence must not invent a fractional tail.
        for rounded in [
            &mut result.in_range.rounded,
            &mut result.masked,
            &mut result.adjusted,
        ] {
            rounded.inexact = true.into();
        }
        let zero = y.zero.or(&self.zero);
        let rounded_zero = RoundedValue::zero(negative);
        result.replace_when(&zero, &rounded_zero);
        result.in_range.rounded = rounded_zero.select(&zero, &result.in_range.rounded);
        result.in_range.valid = zero.or(result.in_range.valid);
        result
    }
}

impl Operand {
    fn logarithm_increment_domain(&self) -> Val<I1> {
        let exponent = self.bits.exponent_field();
        // Largest binary80 value no greater than 1 - sqrt(2)/2.
        exponent.unsigned().lt(16381).or(exponent.eq(16381).and(
            self.bits
                .significand
                .unsigned()
                .lt(0x95f6_1998_0c43_36f8_u64),
        ))
    }
}

impl BinaryOperands {
    pub(crate) fn logarithm_operands_valid(&self, argument: LogarithmArgument) -> Val<I1> {
        let domain = match argument {
            LogarithmArgument::Value => self.right.bits.negative().or(&self.right.zero).eq(false),
            LogarithmArgument::Increment => self.right.logarithm_increment_domain(),
        };
        self.normal_or_zero().and(domain)
    }

    pub(crate) fn logarithm_product(
        &self,
        body: &mut BlockBuilder<'_>,
        argument: LogarithmArgument,
        rounding: &RoundingMode,
    ) -> Result<ArithmeticResult, BuildError> {
        match argument {
            LogarithmArgument::Value => self.value_logarithm_product(body, rounding),
            LogarithmArgument::Increment => self.increment_logarithm_product(body, rounding),
        }
    }

    fn value_logarithm_product(
        &self,
        body: &mut BlockBuilder<'_>,
        rounding: &RoundingMode,
    ) -> Result<ArithmeticResult, BuildError> {
        let y = &self.left;
        let x = &self.right;
        let (significand, exponent) = x.normalized();
        let significand = x.zero.select(1_u64 << 63, significand);
        // Center the mantissa on one, so near-one values keep exponent zero
        // and never subtract a rounded logarithm from an integer exponent.
        let halve = significand.unsigned().ge(0xc000_0000_0000_0000_u64);
        let mantissa = DoubleDouble::significand(&significand).scale(&halve.select(-1, 0));
        let exponent = exponent.add(halve.unsigned().extend::<I32>());
        let one_coefficient = DoubleDouble::from_f64(1.0);
        let ratio = mantissa
            .sub(&one_coefficient)
            .divide(&mantissa.add(&one_coefficient));
        let logarithm = atanh_ratio(body, &ratio)?
            .mul(&ratio)
            .scale(&1.into())
            .divide(&DoubleDouble::ln_2())
            .add(&DoubleDouble::from_f64(exponent.signed().to_f64()));
        let negative = logarithm.parts().0.lt(0.0);
        let logarithm = logarithm.neg().select(&negative, &logarithm);
        let one = x
            .bits
            .exponent_field()
            .eq(16383)
            .and(x.bits.significand.eq(1_u64 << 63));
        let mut result = Logarithm {
            coefficient: one_coefficient.select(&one, &logarithm),
            exponent: 0.into(),
            negative,
            zero: one.clone(),
            // Integral logarithms and their products fit exactly in the pair.
            discarded_tail: significand.ne(1_u64 << 63),
        }
        .multiply(y, rounding);

        // Distinguish zero * infinity (#IA), finite nonzero * log(0) (#Z),
        // and infinite y * log(0) (no #Z), as in the architectural table.
        let infinity = x.infinity.or(&x.zero).or(&y.infinity);
        let infinite_negative = y
            .bits
            .negative()
            .xor(x.zero.or(x.bits.exponent_field().unsigned().lt(16383)));
        result.replace_when(&infinity, &RoundedValue::infinity(infinite_negative));
        result.zero_divide = x.zero.and(y.zero.or(&y.infinity).eq(false));
        let invalid = x
            .bits
            .negative()
            .and(x.zero.eq(false))
            .or(y.zero.and(x.zero.or(&x.infinity)))
            .or(y.infinity.and(&one))
            .and(x.nan.or(&y.nan).eq(false));
        Ok(self.finish(result, invalid))
    }

    fn increment_logarithm_product(
        &self,
        body: &mut BlockBuilder<'_>,
        rounding: &RoundingMode,
    ) -> Result<ArithmeticResult, BuildError> {
        let y = &self.left;
        let x = &self.right;
        let (significand, exponent) = x.normalized();
        let coefficient = DoubleDouble::significand(&x.zero.select(1_u64 << 63, significand));
        let linear = coefficient.divide(&DoubleDouble::ln_2());
        let domain = x.logarithm_increment_domain();
        let parts = body.if_value::<DoubleShape>(
            exponent.signed().lt(-120).or(&x.zero).or(domain.eq(false)),
            |body| body.yield_(linear.parts()),
            |mut body| {
                let value = coefficient.scale(&exponent);
                let value = value.neg().select(&x.bits.negative(), &value);
                let two = DoubleDouble::from_f64(2.0);
                let denominator = two.add(&value);
                let ratio = value.divide(&denominator);
                let series = atanh_ratio(&mut body, &ratio)?;
                body.yield_(linear.mul(&two.divide(&denominator)).mul(&series).parts())
            },
        )?;
        // log2(1+x) = x * [2/(2+x)] * atanh_ratio(x/(2+x)) / ln(2).
        // Keeping x factored avoids forming 1+x and retains subnormal exponents.
        let mut result = Logarithm {
            coefficient: DoubleDouble::from_parts(parts),
            exponent,
            negative: x.bits.negative(),
            zero: x.zero.clone(),
            discarded_tail: true.into(),
        }
        .multiply(y, rounding);
        let infinity = y.infinity.and(&domain);
        result.replace_when(
            &infinity,
            &RoundedValue::infinity(y.bits.negative().xor(x.bits.negative())),
        );
        // Outside Intel's finite domain the result is undefined. Retain y and
        // complete the pop as local policy; ordinary NaN/encoding responses remain.
        result.replace_when(
            &domain.eq(false),
            &RoundedValue::exact(ExtendedValue::from_bits(y.bits.clone())),
        );
        let invalid = x.zero.and(&y.infinity).and(x.nan.or(&y.nan).eq(false));
        Ok(self.finish(result, invalid))
    }
}

/// atanh(z)/z = 1 + z²/3 + z⁴/5 + ..., for |z| <= 1/5.
/// The 32-term relative tail is below 2^-148. With contracting terms and a
/// normalized sum in [1, 1.014], accumulated DD roundoff stays below 2^-92.
/// The surrounding divisions and multiplication by y retain a relative error
/// below 2^-88 before the single binary80 rounding.
fn atanh_ratio(
    body: &mut BlockBuilder<'_>,
    argument: &DoubleDouble,
) -> Result<DoubleDouble, BuildError> {
    odd_power_series(body, &argument.mul(argument), 32)
}

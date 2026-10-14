//! F2XM1 factors out x so tiny operands retain their extended exponent.

use wasm86_compiler::{BlockBuilder, BuildError, Val, F64, I32};

use crate::floating::RoundingMode;

use super::{
    result::RoundedValue,
    transcendental::{DoubleDouble, DoubleShape},
    ArithmeticResult, ExtendedBits, ExtendedValue, Operand,
};

impl Operand {
    pub(crate) fn exponential_domain(&self) -> Val<wasm86_compiler::I1> {
        let exponent = self.bits.exponent_field();
        exponent.unsigned().lt(16383).or(exponent
            .eq(16383)
            .and(self.bits.significand.eq(1_u64 << 63)))
    }

    pub(crate) fn exponential_minus_one(
        &self,
        body: &mut BlockBuilder<'_>,
        rounding: &RoundingMode,
    ) -> Result<ArithmeticResult, BuildError> {
        let (significand, exponent) = self.normalized();
        let coefficient = DoubleDouble::significand(&self.zero.select(1_u64 << 63, significand));
        let linear = coefficient.mul(&DoubleDouble::ln_2());
        let domain = self.exponential_domain();
        let tiny = exponent.signed().lt(-120);
        let endpoint = self
            .bits
            .exponent_field()
            .eq(16383)
            .and(self.bits.significand.eq(1_u64 << 63));
        let parts = body.if_value::<DoubleShape>(
            tiny.or(&self.zero).or(&endpoint).or(domain.eq(false)),
            |body| body.yield_(linear.parts()),
            |mut body| {
                let x = coefficient.scale(&exponent);
                let x = x.neg().select(&self.bits.negative(), &x);
                let power = x.mul(&DoubleDouble::ln_2());
                let factor = exponential_ratio(&mut body, &power)?;
                body.yield_(linear.mul(&factor).parts())
            },
        )?;
        // Below 2^-120 the omitted quadratic term is smaller than the working
        // precision. Nonzero inputs retain the transcendental precision response,
        // including endpoints whose final value is exactly representable.
        let mut result = DoubleDouble::from_parts(parts)
            .magnitude(exponent, self.bits.negative(), true.into())
            .round(3.into(), rounding);

        // Match native -infinity -> -1 within the unspecified input domain.
        let negative_infinity = domain
            .eq(false)
            .and(&self.infinity)
            .and(self.bits.negative());
        let mut exact = RoundedValue::exact(ExtendedValue::from_bits(ExtendedBits {
            significand: endpoint.select(1_u64 << 63, &self.bits.significand),
            sign_exponent: endpoint.select(
                self.bits.negative().select(0xbffe_u32, 0x3fff_u32),
                negative_infinity.select(0xbfff_u32, &self.bits.sign_exponent),
            ),
        }));
        exact.inexact = endpoint.clone();
        // Other out-of-domain inputs retain their source as local policy.
        // NaNs and unsupported encodings still use the ordinary operand response.
        let exact_case = self.zero.or(&endpoint).or(domain.eq(false));
        result.replace_when(&exact_case, &exact);
        result.in_range.rounded = exact.select(&exact_case, &result.in_range.rounded);
        result.in_range.valid = exact_case.or(result.in_range.valid);
        Ok(self.finish(result, false.into()))
    }
}

/// expm1(z)/z = 1 + z/2! + z²/3! + ..., for |z| <= ln(2).
/// The 32-term truncation error is below 2^-138. Each recurrence contracts
/// by at least ln(2)/2; the sum stays between 0.72 and 1.45. With binary64
/// unit roundoff u = 2^-53, a conservative 64u² local arithmetic allowance
/// over 32 terms bounds accumulated error below 2^-92. Including the ln(2)
/// approximation and the final product keeps the coefficient error below
/// 2^-88, leaving more than 20 guard bits at binary80 precision.
fn exponential_ratio(
    body: &mut BlockBuilder<'_>,
    argument: &DoubleDouble,
) -> Result<DoubleDouble, BuildError> {
    let parts = body.loop_::<(I32, F64, F64, F64, F64), DoubleShape>(
        (2, 1.0, 0.0, 1.0, 0.0),
        |mut iteration, labels, (index, term_high, term_low, sum_high, sum_low)| {
            iteration.if_(index.eq(33), |done| {
                done.branch(&labels.exit, (&sum_high, &sum_low))
            })?;
            let term = DoubleDouble::from_parts((term_high, term_low))
                .mul(argument)
                .divide(&DoubleDouble::from_f64(index.unsigned().to_f64()));
            let sum = DoubleDouble::from_parts((sum_high, sum_low)).add(&term);
            let (term_high, term_low) = term.parts();
            let (sum_high, sum_low) = sum.parts();
            iteration.branch(
                &labels.again,
                (index.add(1), term_high, term_low, sum_high, sum_low),
            )
        },
    )?;
    Ok(DoubleDouble::from_parts(parts))
}

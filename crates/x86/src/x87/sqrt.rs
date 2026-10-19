//! Square root keeps exact rounding evidence at every x87 precision.

mod native;

use relocade_compiler::{BlockBuilder, BuildError, Val, I8};

use crate::floating::{FiniteInput, RoundingMode};

use super::{
    result::{RoundedShape, RoundedValue},
    value::KnownClass,
    ArithmeticResult, ExtendedValue, Operand,
};

impl Operand {
    pub(crate) fn square_root(
        &self,
        body: &mut BlockBuilder<'_>,
        precision: Val<I8>,
        rounding: &RoundingMode,
    ) -> Result<ArithmeticResult, BuildError> {
        let rounded = self.rounded_square_root(body, precision, rounding)?;
        let mut result = ArithmeticResult::from_rounding(rounded);
        result.replace_when(
            &self.infinity,
            &RoundedValue::exact(ExtendedValue::from_bits(self.bits.clone())),
        );
        let negative = self.bits.negative().and(self.zero.or(&self.nan).eq(false));
        Ok(self.finish(result, negative))
    }

    /// Admission excludes negative nonzero and exceptional operands. Every
    /// finite square root fits the extended normal range, so only #P remains.
    pub(crate) fn square_root_candidate(
        &self,
        body: &mut BlockBuilder<'_>,
        precision: Val<I8>,
        rounding: &RoundingMode,
        nearest_53: bool,
    ) -> Result<RoundedValue, BuildError> {
        if nearest_53 {
            if let Some(rounded) = native::calculate(self) {
                return Ok(rounded);
            }
            // The control guard proves these construction-time constants.
            // Full-width input still needs exact refinement before rounding.
            return self.rounded_square_root(body, 2.into(), &RoundingMode::new(0.into()));
        }
        self.rounded_square_root(body, precision, rounding)
    }

    fn rounded_square_root(
        &self,
        body: &mut BlockBuilder<'_>,
        precision: Val<I8>,
        rounding: &RoundingMode,
    ) -> Result<RoundedValue, BuildError> {
        let components = body.if_value::<RoundedShape>(
            &self.zero,
            |body| body.yield_(RoundedValue::zero(self.bits.negative()).components()),
            |mut body| {
                let (significand, exponent) = self.normalized();
                let magnitude = FiniteInput {
                    significand,
                    exponent,
                    negative: false.into(),
                }
                .square_root(&mut body)?;
                body.yield_(
                    magnitude
                        .round(precision.clone(), rounding)
                        .in_range
                        .rounded
                        .components(),
                )
            },
        )?;
        let mut rounded = RoundedValue::from_components(
            components,
            KnownClass::zero().select(&self.zero, &KnownClass::normal()),
        );
        if matches!(body.constant_bits(precision.and(3))?, Some(0) | Some(2)) {
            rounded.value = rounded.value.assume_precision53();
        }
        Ok(rounded)
    }
}

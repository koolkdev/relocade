//! Integer rounding checks the destination range after rounding. Native binary64
//! and full-width significands retain their respective exact calculation paths.

use super::{FiniteMagnitude, RoundingMode};
use wasm86_compiler::{Val, ValueType, F64, I1, I32, I64};

/// An integer candidate, its range check and unfiltered rounding evidence.
/// Owners suppress precision and increment flags for invalid source encodings
/// or destinations after combining their own validity conditions.
pub(crate) struct IntegerRounded<T: ValueType> {
    pub(crate) value: Val<T>,
    pub(crate) invalid: Val<I1>,
    pub(crate) inexact: Val<I1>,
    pub(crate) incremented: Val<I1>,
}

impl RoundingMode {
    /// Binary32/64 inputs can use native integral rounding without losing bits.
    /// Ordered comparisons reject NaNs and infinities as well as overflow.
    pub(crate) fn signed_dword(&self, value: &Val<F64>) -> IntegerRounded<I32> {
        let rounded = self.integral(value);
        let valid = rounded.ge(-2147483648.0).and(rounded.lt(2147483648.0));
        IntegerRounded {
            value: rounded.trunc_saturating_i32(),
            invalid: valid.eq(false),
            inexact: value.ne(&rounded),
            incremented: rounded.abs().gt(value.abs()),
        }
    }
}

impl FiniteMagnitude {
    /// The caller supplies a magnitude limit and handles unsupported source
    /// encodings, sign insertion and the destination's masked-invalid encoding.
    pub(crate) fn round_integer(
        &self,
        rounding: &RoundingMode,
        limit: &Val<I64>,
    ) -> IntegerRounded<I64> {
        let too_large = self.exponent.signed().ge(64);
        let distance = too_large.select(0, Val::<I32>::from(63).sub(&self.exponent));
        let input = self.significand.shift_right(&distance);
        let truncated = input.integer.clone();
        let rounded = rounding.round(input, &self.negative);
        // A right shift leaves room for a carry. With no shift, only a prior
        // fractional remainder can push a full-width integer past u64::MAX.
        let carry = self
            .significand
            .guard
            .or(&self.significand.sticky)
            .and(distance.eq(0))
            .and(rounded.integer.unsigned().lt(truncated));
        let invalid = too_large
            .or(carry)
            .or(limit.unsigned().lt(&rounded.integer));
        IntegerRounded {
            value: rounded.integer,
            invalid,
            inexact: rounded.inexact,
            incremented: rounded.incremented,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{floating::RoundingInput, test_step as step, CompiledModule};
    use wasm86_compiler::{Program, Signature, Type};

    #[test]
    fn integer_rounding_rejects_carry_beyond_u64() {
        let mut program = Program::new();
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I1],
                    results: vec![Type::I1, Type::I1, Type::I1],
                },
                |body| {
                    let magnitude = FiniteMagnitude {
                        significand: RoundingInput {
                            integer: u64::MAX.into(),
                            guard: body.parameter::<I1>(0)?,
                            sticky: false.into(),
                        },
                        exponent: 63.into(),
                        negative: false.into(),
                    };
                    let result =
                        magnitude.round_integer(&RoundingMode::new(0.into()), &u64::MAX.into());
                    body.return_((result.invalid, result.inexact, result.incremented))
                },
            )
            .unwrap();
        program.export("run", function).unwrap();
        let module = step::TestModule::new(&CompiledModule {
            bytes: program.compile().unwrap(),
            entry: "run".into(),
            execution_profile: None,
        });
        for guard in [0, 1] {
            let input = step::Input {
                arguments: vec![step::Argument::I32(guard)],
                ..step::Input::new(&[])
            };
            assert_eq!(
                module.observe(&input, 1).events,
                vec![step::Event::Return {
                    outcome: step::Outcome::Returned(vec![
                        step::Argument::I32(guard),
                        step::Argument::I32(guard),
                        step::Argument::I32(guard)
                    ]),
                    snapshot: step::Snapshot {
                        cpu: vec![],
                        guest: None
                    },
                }]
            );
        }
    }
}

//! Arithmetic register destinations share exception, rounding, and stack effects.

use relocade_compiler::{BuildError, Val, I1, I32};

use crate::{
    execution::ExecutionBuilder,
    state::x87::{ConditionCode, Exception, StackValue},
    state::X87Access,
    x87::{ArithmeticResult, ExtendedValue},
};

pub(super) enum PairRounding {
    Original,
    Pushed,
}

pub(super) enum Destination {
    Register {
        index: Val<I32>,
        pop: bool,
    },
    Conditional {
        index: Val<I32>,
        when: Val<I1>,
    },
    PushPair {
        when: Val<I1>,
        overflow: Val<I1>,
        rounding: PairRounding,
    },
}

impl Destination {
    fn rounding_result(&self) -> usize {
        match self {
            Self::PushPair {
                rounding: PairRounding::Pushed,
                ..
            } => 1,
            _ => 0,
        }
    }

    fn stack_overflow(&self) -> Val<I1> {
        match self {
            Self::PushPair { overflow, .. } => overflow.clone(),
            _ => false.into(),
        }
    }

    fn write<const N: usize>(
        self,
        x87: &mut X87Access<'_, '_>,
        values: [ExtendedValue; N],
        enabled: &Val<I1>,
    ) -> Result<(), BuildError> {
        match self {
            Self::Register { index, pop } => {
                assert_eq!(N, 1);
                x87.write_stack(index, &StackValue::from_value(values[0].clone()), enabled)?;
                if pop {
                    x87.pop(enabled)?;
                }
                Ok(())
            }
            Self::Conditional { index, when } => {
                assert_eq!(N, 1);
                x87.write_stack_when(
                    index,
                    &StackValue::from_value(values[0].clone()),
                    &when,
                    enabled,
                )
            }
            Self::PushPair { when, .. } => {
                assert_eq!(N, 2);
                // Both values use entry TOP; the push owns the final movement.
                x87.write_stack_when(
                    0,
                    &StackValue::from_value(values[0].clone()),
                    &when,
                    enabled,
                )?;
                x87.push_when(&values[1], &when, enabled)
            }
        }
    }
}

/// Applies one instruction's exception response to one register or a pushed pair.
/// Each magnitude resolves its own range response; status/pending flags aggregate
/// once. Unmasked operand exceptions suppress all writes and stack movement.
pub(super) fn write_results<const N: usize>(
    execution: &mut ExecutionBuilder<'_, '_>,
    results: [ArithmeticResult; N],
    stack_fault: &Val<I1>,
    destination: Destination,
) -> Result<(), BuildError> {
    let mut x87 = execution.x87();
    let results = results.map(|result| result.or_indefinite(stack_fault));
    let any = |field: fn(&ArithmeticResult) -> &Val<I1>| {
        results.iter().fold(Val::<I1>::from(false), |flag, result| {
            flag.or(field(result))
        })
    };
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &any(|r| &r.invalid))?;
    let unmasked_zero_divide =
        x87.record_exception(Exception::ZeroDivide, &any(|r| &r.zero_divide))?;
    let unmasked_denormal = x87.record_exception(Exception::Denormal, &any(|r| &r.denormal))?;
    x87.record_stack_fault(stack_fault)?;
    let suppressed = unmasked_invalid
        .or(unmasked_zero_divide)
        .or(unmasked_denormal);
    let enabled = suppressed.eq(false);
    let overflow = enabled.and(any(|r| &r.overflow));
    let unmasked_overflow = x87.record_exception(Exception::Overflow, &overflow)?;
    let underflow_unmasked = x87.exception_unmasked(Exception::Underflow)?;
    let rounded = results.each_ref().map(|result| {
        result.resolve_range(
            &unmasked_overflow
                .and(&result.overflow)
                .or(enabled.and(&result.tiny).and(&underflow_unmasked)),
        )
    });
    let underflow =
        results
            .iter()
            .zip(&rounded)
            .fold(Val::<I1>::from(false), |flag, (result, rounded)| {
                flag.or(enabled
                    .and(&result.tiny)
                    .and(underflow_unmasked.or(&rounded.inexact)))
            });
    let unmasked_underflow = x87.record_exception(Exception::Underflow, &underflow)?;
    let precision = enabled.and(
        rounded
            .iter()
            .fold(Val::<I1>::from(false), |flag, r| flag.or(&r.inexact)),
    );
    let unmasked_precision = x87.record_exception(Exception::Precision, &precision)?;
    x87.set_condition_code(
        ConditionCode::C1,
        destination
            .stack_overflow()
            .or(enabled.and(&rounded[destination.rounding_result()].incremented)),
    )?;
    x87.record_pending_exception(
        suppressed
            .or(unmasked_overflow)
            .or(unmasked_underflow)
            .or(unmasked_precision),
    )?;
    // Register range and precision exceptions still publish both results and
    // the stack movement; only operand exceptions suppress those effects.
    destination.write(&mut x87, rounded.map(|result| result.value), &enabled)
}

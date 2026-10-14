//! Trigonometric definitions own range flags and register destination policy.

use super::*;
use crate::{
    state::x87::{ConditionCode, X87Specialization},
    x87::{ArithmeticResult, Operand, TrigonometricArgument, TrigonometricFunction},
};
use wasm86_compiler::I1;

#[derive(Clone, Copy)]
enum Operation {
    Single(TrigonometricFunction),
    SineCosine,
    Tangent,
}

instruction_families! {
    FSIN {
        execute: trigonometric(Operation::Single(TrigonometricFunction::Sine));
        forms { 0xD9 @ 0xFE => no_operands(); }
    }
    FCOS {
        execute: trigonometric(Operation::Single(TrigonometricFunction::Cosine));
        forms { 0xD9 @ 0xFF => no_operands(); }
    }
    FPTAN {
        execute: trigonometric(Operation::Tangent);
        forms { 0xD9 @ 0xF2 => no_operands(); }
    }
    FSINCOS {
        execute: trigonometric(Operation::SineCosine);
        forms { 0xD9 @ 0xFB => no_operands(); }
    }
}

fn trigonometric(
    execution: &mut ExecutionBuilder<'_, '_>,
    operation: Operation,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let top = execution.x87().read_stack(0)?;
    let empty = top.is_empty();
    let full = match operation {
        Operation::Single(_) => false.into(),
        Operation::SineCosine | Operation::Tangent => execution.x87().push_full()?,
    };
    let mut fault = empty.or(&full);
    let mut overflow = empty.eq(false).and(full);
    let operand = Operand::new(&top.value);
    let rounding = execution.x87().rounding()?;
    execution.specialize_x87(X87Specialization::Rounding)?;
    let mut outside = operand.trigonometric_out_of_range();
    if execution.specialize_x87_operands(
        &mut fault,
        operand.normal_or_zero().and(operand.trigonometric_domain()),
    )? {
        outside = false.into();
        overflow = false.into();
    }
    let argument = execution.calculate(|body| TrigonometricArgument::new(body, &operand))?;
    let when = outside.eq(false).or(&fault);
    let c2 = fault.eq(false).and(outside);
    match operation {
        Operation::Single(function) => {
            let result =
                execution.calculate(|body| argument.evaluate(body, function, &rounding))?;
            write_trigonometric_results(
                execution,
                [result],
                &fault,
                c2,
                result::Destination::Conditional {
                    index: 0.into(),
                    when,
                },
            )
        }
        Operation::SineCosine | Operation::Tangent => {
            let results = execution.calculate(|body| match operation {
                Operation::Tangent => argument.tangent(body, &rounding),
                _ => Ok([
                    argument.evaluate(body, TrigonometricFunction::Sine, &rounding)?,
                    argument.evaluate(body, TrigonometricFunction::Cosine, &rounding)?,
                ]),
            })?;
            write_trigonometric_results(
                execution,
                results,
                &fault,
                c2,
                result::Destination::PushPair {
                    when,
                    overflow,
                    rounding: match operation {
                        Operation::Tangent => result::PairRounding::Original,
                        _ => result::PairRounding::Pushed,
                    },
                },
            )
        }
    }
}

/// Admission of every component precedes metadata, C2, and stack effects.
fn write_trigonometric_results<const N: usize>(
    execution: &mut ExecutionBuilder<'_, '_>,
    mut results: [ArithmeticResult; N],
    fault: &Val<I1>,
    c2: Val<I1>,
    destination: result::Destination,
) -> Result<(), BuildError> {
    execution.specialize(|execution| {
        for result in &mut results {
            *result = execution.admit_x87_candidate(result.in_range.clone())?;
        }
        Ok(())
    })?;
    execution.record_x87_instruction()?;
    execution.x87().set_condition_code(ConditionCode::C2, c2)?;
    result::write_results(execution, results, fault, destination)
}

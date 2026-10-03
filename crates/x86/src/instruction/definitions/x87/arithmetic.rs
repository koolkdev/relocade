//! Register arithmetic captures both operands before an optional pop.

use super::*;
use crate::{
    instruction::X87StackIndex,
    x87::{self, BinaryOperation},
};

instruction_families! {
    FADD_TOP {
        execute: binary_register(BinaryOperation::Add, Destination::Top, false);
        forms { 0xD8 @ 0xC0 + rm => operands(st); }
    }
    FADD_REGISTER {
        execute: binary_register(BinaryOperation::Add, Destination::Other, false);
        forms { 0xDC @ 0xC0 + rm => operands(st); }
    }
    FADDP_REGISTER {
        execute: binary_register(BinaryOperation::Add, Destination::Other, true);
        forms { 0xDE @ 0xC0 + rm => operands(st); }
    }
    FSUB_TOP {
        execute: binary_register(BinaryOperation::Subtract, Destination::Top, false);
        forms { 0xD8 @ 0xE0 + rm => operands(st); }
    }
    FSUB_REGISTER {
        execute: binary_register(BinaryOperation::Subtract, Destination::Other, false);
        forms { 0xDC @ 0xE8 + rm => operands(st); }
    }
    FSUBP_REGISTER {
        execute: binary_register(BinaryOperation::Subtract, Destination::Other, true);
        forms { 0xDE @ 0xE8 + rm => operands(st); }
    }
    FSUBR_TOP {
        execute: binary_register(BinaryOperation::ReverseSubtract, Destination::Top, false);
        forms { 0xD8 @ 0xE8 + rm => operands(st); }
    }
    FSUBR_REGISTER {
        execute: binary_register(BinaryOperation::ReverseSubtract, Destination::Other, false);
        forms { 0xDC @ 0xE0 + rm => operands(st); }
    }
    FSUBRP_REGISTER {
        execute: binary_register(BinaryOperation::ReverseSubtract, Destination::Other, true);
        forms { 0xDE @ 0xE0 + rm => operands(st); }
    }
    FMUL_TOP {
        execute: binary_register(BinaryOperation::Multiply, Destination::Top, false);
        forms { 0xD8 @ 0xC8 + rm => operands(st); }
    }
    FMUL_REGISTER {
        execute: binary_register(BinaryOperation::Multiply, Destination::Other, false);
        forms { 0xDC @ 0xC8 + rm => operands(st); }
    }
    FMULP_REGISTER {
        execute: binary_register(BinaryOperation::Multiply, Destination::Other, true);
        forms { 0xDE @ 0xC8 + rm => operands(st); }
    }
}

enum Destination {
    Top,
    Other,
}

fn binary_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    other: X87StackIndex,
    operation: BinaryOperation,
    destination: Destination,
    pop: bool,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let (destination, source) = match destination {
        Destination::Top => (0.into(), other.offset()),
        Destination::Other => (other.offset(), 0.into()),
    };
    let mut arithmetic = execution
        .x87()
        .prepare_binary_register(destination, source, pop)?;
    execution.specialize(|jit| {
        jit.specialize_on(arithmetic.precision_only_operands())?;
        arithmetic.assume_present();
        Ok(())
    })?;
    let mut calculation = execution.compute(|body| arithmetic.calculate(body, operation))?;
    execution.specialize(|jit| {
        let candidate = jit.compute(|body| calculation.rounding_candidate(body))?;
        // Operand admission precedes arithmetic; this guard excludes result
        // range exceptions while retaining exact-zero results.
        jit.specialize_on(candidate.valid)?;
        calculation.result = x87::ArithmeticResult::from_rounding(candidate.rounded);
        Ok(())
    })?;
    execution.record_x87_instruction()?;
    execution
        .x87()
        .commit_arithmetic(arithmetic, calculation.result)
}

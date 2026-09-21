//! Decimal adjustments have fixed AL/AX operands in either code size.

use super::*;
use crate::{alu, alu::ArithmeticOp, exception::Exception, flags::Flag};

instruction_families! {
    AAA {
        execute: adjust_unpacked(ArithmeticOp::Add);
        forms { 0x37 => word(AX); }
    }
    AAS {
        execute: adjust_unpacked(ArithmeticOp::Subtract);
        forms { 0x3F => word(AX); }
    }
    DAA {
        execute: adjust_packed(ArithmeticOp::Add);
        forms { 0x27 => byte(AL); }
    }
    DAS {
        execute: adjust_packed(ArithmeticOp::Subtract);
        forms { 0x2F => byte(AL); }
    }
    AAM {
        execute: adjust_after_multiply;
        forms { 0xD4 => word(AX, AL, imm8); }
    }
    AAD {
        execute: adjust_before_division;
        forms { 0xD5 => word(AX, imm8); }
    }
}

fn adjust_unpacked(
    execution: &mut ExecutionBuilder<'_, '_>,
    accumulator: TypedLocation<I16>,
    operation: ArithmeticOp,
) -> Result<(), BuildError> {
    accumulator.update(execution, |execution, value| {
        let auxiliary = execution.read_flag(Flag::AF)?;
        let outcome = operation.adjust_unpacked(value, auxiliary);
        execution.write_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

fn adjust_packed(
    execution: &mut ExecutionBuilder<'_, '_>,
    accumulator: TypedLocation<I8>,
    operation: ArithmeticOp,
) -> Result<(), BuildError> {
    accumulator.update(execution, |execution, value| {
        let [auxiliary, carry] = execution.read_flags([Flag::AF, Flag::CF])?;
        let outcome = operation.adjust_packed(value, auxiliary, carry);
        execution.write_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

fn adjust_after_multiply(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I16>,
    source: Input<I8>,
    base: Input<I8>,
) -> Result<(), BuildError> {
    let base = base.read(execution)?;
    execution.fault_if(base.eq(0), Exception::DivideError)?;
    let outcome = alu::adjust_after_multiply(source.read(execution)?, base);
    execution.write_flags(outcome.flags)?;
    destination.write(execution, outcome.result)
}

fn adjust_before_division(
    execution: &mut ExecutionBuilder<'_, '_>,
    accumulator: TypedLocation<I16>,
    base: Input<I8>,
) -> Result<(), BuildError> {
    accumulator.update(execution, |execution, value| {
        let outcome = alu::adjust_before_division(value, base.read(execution)?);
        execution.write_flags(outcome.flags)?;
        Ok(outcome.result)
    })
}

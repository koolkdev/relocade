//! Binary arithmetic combines an x87 destination with a register, real or integer source.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::{X87MemoryFormat, X87Operand, X87Operands},
    instruction::X87StackIndex,
    state::x87::{ConditionCode, Exception, StackValue, X87Specialization},
    x87::{ArithmeticResult, BinaryFormat, BinaryOperation},
};

instruction_families! {
    FADD_BINARY32 {
        execute: binary_memory(BinaryOperation::Add, X87MemoryFormat::Binary(BinaryFormat::Binary32));
        forms { 0xD8 / 0 => operands(mem); }
    }
    FADD_BINARY64 {
        execute: binary_memory(BinaryOperation::Add, X87MemoryFormat::Binary(BinaryFormat::Binary64));
        forms { 0xDC / 0 => operands(mem); }
    }
    FSUB_BINARY32 {
        execute: binary_memory(BinaryOperation::Subtract, X87MemoryFormat::Binary(BinaryFormat::Binary32));
        forms { 0xD8 / 4 => operands(mem); }
    }
    FSUB_BINARY64 {
        execute: binary_memory(BinaryOperation::Subtract, X87MemoryFormat::Binary(BinaryFormat::Binary64));
        forms { 0xDC / 4 => operands(mem); }
    }
    FSUBR_BINARY32 {
        execute: binary_memory(BinaryOperation::ReverseSubtract, X87MemoryFormat::Binary(BinaryFormat::Binary32));
        forms { 0xD8 / 5 => operands(mem); }
    }
    FSUBR_BINARY64 {
        execute: binary_memory(BinaryOperation::ReverseSubtract, X87MemoryFormat::Binary(BinaryFormat::Binary64));
        forms { 0xDC / 5 => operands(mem); }
    }
    FMUL_BINARY32 {
        execute: binary_memory(BinaryOperation::Multiply, X87MemoryFormat::Binary(BinaryFormat::Binary32));
        forms { 0xD8 / 1 => operands(mem); }
    }
    FMUL_BINARY64 {
        execute: binary_memory(BinaryOperation::Multiply, X87MemoryFormat::Binary(BinaryFormat::Binary64));
        forms { 0xDC / 1 => operands(mem); }
    }
    FDIV_BINARY32 {
        execute: binary_memory(BinaryOperation::Divide, X87MemoryFormat::Binary(BinaryFormat::Binary32));
        forms { 0xD8 / 6 => operands(mem); }
    }
    FDIV_BINARY64 {
        execute: binary_memory(BinaryOperation::Divide, X87MemoryFormat::Binary(BinaryFormat::Binary64));
        forms { 0xDC / 6 => operands(mem); }
    }
    FDIVR_BINARY32 {
        execute: binary_memory(BinaryOperation::ReverseDivide, X87MemoryFormat::Binary(BinaryFormat::Binary32));
        forms { 0xD8 / 7 => operands(mem); }
    }
    FDIVR_BINARY64 {
        execute: binary_memory(BinaryOperation::ReverseDivide, X87MemoryFormat::Binary(BinaryFormat::Binary64));
        forms { 0xDC / 7 => operands(mem); }
    }
    FIADD_INTEGER16 {
        execute: binary_memory(BinaryOperation::Add, X87MemoryFormat::Integer16);
        forms { 0xDE / 0 => operands(mem); }
    }
    FIADD_INTEGER32 {
        execute: binary_memory(BinaryOperation::Add, X87MemoryFormat::Integer32);
        forms { 0xDA / 0 => operands(mem); }
    }
    FIMUL_INTEGER16 {
        execute: binary_memory(BinaryOperation::Multiply, X87MemoryFormat::Integer16);
        forms { 0xDE / 1 => operands(mem); }
    }
    FIMUL_INTEGER32 {
        execute: binary_memory(BinaryOperation::Multiply, X87MemoryFormat::Integer32);
        forms { 0xDA / 1 => operands(mem); }
    }
    FISUB_INTEGER16 {
        execute: binary_memory(BinaryOperation::Subtract, X87MemoryFormat::Integer16);
        forms { 0xDE / 4 => operands(mem); }
    }
    FISUB_INTEGER32 {
        execute: binary_memory(BinaryOperation::Subtract, X87MemoryFormat::Integer32);
        forms { 0xDA / 4 => operands(mem); }
    }
    FISUBR_INTEGER16 {
        execute: binary_memory(BinaryOperation::ReverseSubtract, X87MemoryFormat::Integer16);
        forms { 0xDE / 5 => operands(mem); }
    }
    FISUBR_INTEGER32 {
        execute: binary_memory(BinaryOperation::ReverseSubtract, X87MemoryFormat::Integer32);
        forms { 0xDA / 5 => operands(mem); }
    }
    FIDIV_INTEGER16 {
        execute: binary_memory(BinaryOperation::Divide, X87MemoryFormat::Integer16);
        forms { 0xDE / 6 => operands(mem); }
    }
    FIDIV_INTEGER32 {
        execute: binary_memory(BinaryOperation::Divide, X87MemoryFormat::Integer32);
        forms { 0xDA / 6 => operands(mem); }
    }
    FIDIVR_INTEGER16 {
        execute: binary_memory(BinaryOperation::ReverseDivide, X87MemoryFormat::Integer16);
        forms { 0xDE / 7 => operands(mem); }
    }
    FIDIVR_INTEGER32 {
        execute: binary_memory(BinaryOperation::ReverseDivide, X87MemoryFormat::Integer32);
        forms { 0xDA / 7 => operands(mem); }
    }
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
    FDIV_TOP {
        execute: binary_register(BinaryOperation::Divide, Destination::Top, false);
        forms { 0xD8 @ 0xF0 + rm => operands(st); }
    }
    FDIV_REGISTER {
        execute: binary_register(BinaryOperation::Divide, Destination::Other, false);
        forms { 0xDC @ 0xF8 + rm => operands(st); }
    }
    FDIVP_REGISTER {
        execute: binary_register(BinaryOperation::Divide, Destination::Other, true);
        forms { 0xDE @ 0xF8 + rm => operands(st); }
    }
    FDIVR_TOP {
        execute: binary_register(BinaryOperation::ReverseDivide, Destination::Top, false);
        forms { 0xD8 @ 0xF8 + rm => operands(st); }
    }
    FDIVR_REGISTER {
        execute: binary_register(BinaryOperation::ReverseDivide, Destination::Other, false);
        forms { 0xDC @ 0xF0 + rm => operands(st); }
    }
    FDIVRP_REGISTER {
        execute: binary_register(BinaryOperation::ReverseDivide, Destination::Other, true);
        forms { 0xDE @ 0xF0 + rm => operands(st); }
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
    let (destination, source) = match destination {
        Destination::Top => (0.into(), other.offset()),
        Destination::Other => (other.offset(), 0.into()),
    };
    arithmetic(
        execution,
        destination,
        X87Operand::Register(source),
        operation,
        pop,
    )
}

fn binary_memory(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    operation: BinaryOperation,
    format: X87MemoryFormat,
) -> Result<(), BuildError> {
    arithmetic(
        execution,
        0.into(),
        X87Operand::Memory { address, format },
        operation,
        false,
    )
}

fn arithmetic(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: Val<I32>,
    source: X87Operand,
    operation: BinaryOperation,
    pop: bool,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let X87Operands {
        values: operands,
        mut stack_fault,
        memory,
    } = execution.read_x87_operands(destination.clone(), source)?;
    let precision = execution.x87().precision()?;
    let rounding = execution.x87().rounding()?;
    let specialized = execution.specialize(|execution| {
        execution.specialize_x87(X87Specialization::Arithmetic)?;
        execution.specialize_on(
            stack_fault
                .eq(false)
                .and(operands.precision_only(operation)),
        )?;
        stack_fault = false.into();
        // The preceding control guard validates these observed fields.
        let nearest_53 = execution.observed_cpu().is_some_and(|observed| {
            let control = &observed.x87.control;
            control.precision_control & 3 == 2 && control.rounding_control & 3 == 0
        });
        let candidate = execution.calculate(|body| {
            operands.rounding_candidate(body, operation, precision.clone(), &rounding, nearest_53)
        })?;
        execution.specialize_on(candidate.valid)?;
        Ok(ArithmeticResult::from_rounding(candidate.rounded))
    })?;
    let result = match specialized {
        Some(result) => result,
        None => execution.calculate(|body| {
            Ok(operands
                .calculate(body, operation, precision, &rounding)?
                .result)
        })?,
    };
    execution.record_x87_operand(memory.as_ref())?;

    let mut x87 = execution.x87();
    // Stack faults override all numerical responses, including #D from a
    // narrow source that expanded to a normal binary80 value.
    let result = result.or_indefinite(&stack_fault);
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &result.invalid)?;
    let unmasked_zero_divide = x87.record_exception(Exception::ZeroDivide, &result.zero_divide)?;
    let unmasked_denormal = x87.record_exception(Exception::Denormal, &result.denormal)?;
    x87.record_stack_fault(&stack_fault)?;
    let suppressed = unmasked_invalid
        .or(unmasked_zero_divide)
        .or(unmasked_denormal);
    let enabled = suppressed.eq(false);
    let overflow = enabled.and(&result.overflow);
    let unmasked_overflow = x87.record_exception(Exception::Overflow, &overflow)?;
    let tiny = enabled.and(&result.tiny);
    let underflow_unmasked = x87.exception_unmasked(Exception::Underflow)?;
    let rounded = result.resolve_range(&unmasked_overflow.or(tiny.and(&underflow_unmasked)));
    let underflow = tiny.and(underflow_unmasked.or(&rounded.inexact));
    let unmasked_underflow = x87.record_exception(Exception::Underflow, &underflow)?;
    let precision = enabled.and(rounded.inexact);
    let unmasked_precision = x87.record_exception(Exception::Precision, &precision)?;
    x87.set_condition_code(ConditionCode::C1, enabled.and(rounded.incremented))?;
    x87.record_pending_exception(
        suppressed
            .or(unmasked_overflow)
            .or(unmasked_underflow)
            .or(unmasked_precision),
    )?;
    // Unlike memory stores, unmasked range and precision exceptions write
    // register results, including the pop and any adjusted exponent.
    x87.write_stack(
        destination,
        &StackValue::from_value(rounded.value),
        &enabled,
    )?;
    if pop {
        x87.pop(&enabled)?;
    }
    Ok(())
}

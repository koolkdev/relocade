//! x87 stores check the destination before conversion or stack effects.

mod format;

use super::*;
use crate::{
    address::MemoryAddress,
    floating::{BinaryFormat, RoundingMode},
    instruction::X87StackIndex,
    memory::Intent,
    state::x87::{ConditionCode, Exception, StackValue, X87Specialization},
};
use format::{PackedDecimal, SignedInteger, StoreFormat};
use wasm86_compiler::{MemoryInt, I64};

#[derive(Clone, Copy)]
enum StoreRounding {
    Control,
    Truncate,
}

instruction_families! {
    FST_BINARY32 {
        execute: store(BinaryFormat::Binary32, false, StoreRounding::Control);
        requires: [Fpu];
        forms { 0xD9 / 2 => operands(mem); }
    }
    FSTP_BINARY32 {
        execute: store(BinaryFormat::Binary32, true, StoreRounding::Control);
        requires: [Fpu];
        forms { 0xD9 / 3 => operands(mem); }
    }
    FST_BINARY64 {
        execute: store(BinaryFormat::Binary64, false, StoreRounding::Control);
        requires: [Fpu];
        forms { 0xDD / 2 => operands(mem); }
    }
    FSTP_BINARY64 {
        execute: store(BinaryFormat::Binary64, true, StoreRounding::Control);
        requires: [Fpu];
        forms { 0xDD / 3 => operands(mem); }
    }
    FSTP_EXTENDED {
        execute: store_extended;
        requires: [Fpu];
        forms { 0xDB / 7 => operands(mem); }
    }
    FIST {
        execute: store_integer::<_>(false, StoreRounding::Control);
        requires: [Fpu];
        forms {
            0xDF / 2 => word(mem);
            0xDB / 2 => dword(mem);
        }
    }
    FISTP {
        execute: store_integer::<_>(true, StoreRounding::Control);
        requires: [Fpu];
        forms {
            0xDF / 3 => word(mem);
            0xDB / 3 => dword(mem);
            0xDF / 7 => qword(mem);
        }
    }
    FISTTP {
        execute: store_integer::<_>(true, StoreRounding::Truncate);
        requires: [Fpu, Sse3];
        forms {
            0xDF / 1 => word(mem);
            0xDB / 1 => dword(mem);
            0xDD / 1 => qword(mem);
        }
    }
    FBSTP {
        execute: store(PackedDecimal, true, StoreRounding::Control);
        requires: [Fpu];
        forms { 0xDF / 6 => operands(mem); }
    }
    FST_REGISTER {
        execute: store_register(false);
        requires: [Fpu];
        forms { 0xDD @ 0xD0 + rm => operands(st); }
    }
    FSTP_REGISTER {
        execute: store_register(true);
        requires: [Fpu];
        forms { 0xDD @ 0xD8 + rm => operands(st); }
    }
}

fn store_integer<T: MemoryInt + crate::memory::TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    pop: bool,
    rounding: StoreRounding,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    store(
        execution,
        address,
        SignedInteger::<T>::default(),
        pop,
        rounding,
    )
}

fn store<F: StoreFormat>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: F,
    pop: bool,
    rounding: StoreRounding,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, format.bytes(), Intent::Write, &[])?;
    if matches!(rounding, StoreRounding::Control) {
        execution.specialize_x87(X87Specialization::Rounding)?;
    }
    execution.record_x87_memory(&operand)?;
    let mut x87 = execution.x87();
    let source = x87.read_stack(0)?;
    let rounding = match rounding {
        StoreRounding::Control => x87.rounding()?,
        StoreRounding::Truncate => RoundingMode::new(3.into()),
    };
    let result = format.convert(&source.value.or_indefinite(&source.is_empty()), &rounding);
    let invalid = source.is_empty().or(result.invalid);
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &invalid)?;
    x87.record_stack_fault(&source.is_empty())?;
    let unmasked_overflow = x87.record_exception(Exception::Overflow, &result.overflow)?;
    let unmasked_underflow = x87.exception_unmasked(Exception::Underflow)?;
    let underflow = result.tiny.and(unmasked_underflow.or(&result.inexact));
    let unmasked_underflow = x87.record_exception(Exception::Underflow, &underflow)?;
    let suppressed = unmasked_invalid
        .or(unmasked_overflow)
        .or(unmasked_underflow);
    let enabled = suppressed.eq(false);
    // Unmasked range exceptions suppress new PE and clear C1 for memory
    // destinations. Existing sticky exception flags remain untouched.
    let precision = enabled.and(result.inexact);
    let unmasked_precision = x87.record_exception(Exception::Precision, &precision)?;
    x87.set_condition_code(ConditionCode::C1, enabled.and(result.incremented))?;
    x87.record_pending_exception(suppressed.or(unmasked_precision))?;
    if pop {
        x87.pop(&enabled)?;
    }

    execution.if_value::<()>(
        &enabled,
        |arm| format.write(&operand, arm, &result.bits),
        |_| Ok(()),
    )
}

fn store_extended(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, 10, Intent::Write, &[])?;
    let source = execution.x87().read_stack(0)?;
    execution.record_x87_memory(&operand)?;
    let enabled = execution.x87().stack_underflow(&source.is_empty())?;
    execution
        .x87()
        .set_condition_code(ConditionCode::C1, false)?;
    let value = source.value.or_indefinite(&source.is_empty()).bits();
    execution.if_value::<()>(
        &enabled,
        |arm| {
            operand.write(arm, 0, &value.significand)?;
            operand.write(arm, 8, &value.sign_exponent)
        },
        |_| Ok(()),
    )?;
    execution.x87().pop(&enabled)
}

fn store_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: X87StackIndex,
    pop: bool,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    let source = execution.x87().read_stack(0)?;
    let enabled = execution.x87().stack_underflow(&source.is_empty())?;
    execution
        .x87()
        .set_condition_code(ConditionCode::C1, false)?;
    execution.x87().write_stack(
        destination.offset(),
        &StackValue::from_value(source.value.or_indefinite(&source.is_empty())),
        &enabled,
    )?;
    if pop {
        execution.x87().pop(&enabled)?;
    }
    Ok(())
}

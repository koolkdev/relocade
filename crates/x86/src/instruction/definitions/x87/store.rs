//! x87 stores check the destination before conversion or stack effects.

use super::*;
use crate::{
    address::MemoryAddress,
    instruction::X87StackIndex,
    memory::Intent,
    state::x87::{C1Update, StackValue, X87Specialization},
    x87::{BinaryFormat, ConversionResult, ExtendedValue, RoundingMode},
};
use wasm86_compiler::{MemoryInt, I64};

instruction_families! {
    FST_BINARY32 {
        execute: store_binary(BinaryFormat::Binary32, false);
        forms { 0xD9 / 2 => operands(mem); }
    }
    FSTP_BINARY32 {
        execute: store_binary(BinaryFormat::Binary32, true);
        forms { 0xD9 / 3 => operands(mem); }
    }
    FST_BINARY64 {
        execute: store_binary(BinaryFormat::Binary64, false);
        forms { 0xDD / 2 => operands(mem); }
    }
    FSTP_BINARY64 {
        execute: store_binary(BinaryFormat::Binary64, true);
        forms { 0xDD / 3 => operands(mem); }
    }
    FSTP_EXTENDED {
        execute: store_extended;
        forms { 0xDB / 7 => operands(mem); }
    }
    FIST {
        execute: store_integer::<_>(false);
        forms {
            0xDF / 2 => word(mem);
            0xDB / 2 => dword(mem);
        }
    }
    FISTP {
        execute: store_integer::<_>(true);
        forms {
            0xDF / 3 => word(mem);
            0xDB / 3 => dword(mem);
            0xDF / 7 => qword(mem);
        }
    }
    FST_REGISTER {
        execute: store_register(false);
        forms { 0xDD @ 0xD0 + rm => operands(st); }
    }
    FSTP_REGISTER {
        execute: store_register(true);
        forms { 0xDD @ 0xD8 + rm => operands(st); }
    }
}

fn store_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
    pop: bool,
) -> Result<(), BuildError> {
    let convert = |value: &ExtendedValue, rounding: &RoundingMode| format.encode(value, rounding);
    match format {
        BinaryFormat::Binary32 => store::<I32>(execution, address, pop, convert),
        BinaryFormat::Binary64 => store::<I64>(execution, address, pop, convert),
    }
}

fn store_integer<T: MemoryInt + crate::memory::TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    pop: bool,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    store::<T>(
        execution,
        address,
        pop,
        ExtendedValue::to_signed_integer::<T>,
    )
}

fn store<T: MemoryInt + crate::memory::TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    pop: bool,
    convert: impl FnOnce(&ExtendedValue, &RoundingMode) -> ConversionResult,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, T::BYTES, Intent::Write, &[])?;
    execution.specialize(|jit| jit.specialize_x87(X87Specialization::Rounding))?;
    execution.record_x87_memory(&operand)?;
    let store = execution.x87().resolve_store(pop, convert)?;
    execution.if_value::<()>(
        &store.enabled,
        |arm| operand.write(arm, 0, store.bits.truncate::<T>()),
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
    let enabled = execution
        .x87()
        .stack_underflow(&source.is_empty(), C1Update::Clear)?;
    let value = source.value.or_indefinite(&source.is_empty()).bits();
    execution.if_value::<()>(
        &enabled,
        |arm| {
            operand.write(arm, 0, &value.significand)?;
            operand.write(arm, 8, &value.sign_exponent)
        },
        |_| Ok(()),
    )?;
    execution.x87().pop(1, &enabled)
}

fn store_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: X87StackIndex,
    pop: bool,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    let source = execution.x87().read_stack(0)?;
    let enabled = execution
        .x87()
        .stack_underflow(&source.is_empty(), C1Update::Clear)?;
    execution.x87().write_stack(
        destination.offset(),
        &StackValue::from_value(source.value.or_indefinite(&source.is_empty())),
        &enabled,
    )?;
    if pop {
        execution.x87().pop(1, &enabled)?;
    }
    Ok(())
}

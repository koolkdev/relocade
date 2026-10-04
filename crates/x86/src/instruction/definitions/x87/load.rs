//! x87 loads convert memory formats or copy a register before pushing.

use super::*;
use crate::{
    address::MemoryAddress,
    instruction::X87StackIndex,
    memory::Intent,
    state::LoadSource,
    x87::{BinaryFormat, ExtendedBits, ExtendedValue},
};
use wasm86_compiler::{MemoryInt, I64};

instruction_families! {
    FLD_BINARY32 {
        execute: load_binary(BinaryFormat::Binary32);
        forms { 0xD9 / 0 => operands(mem); }
    }
    FLD_BINARY64 {
        execute: load_binary(BinaryFormat::Binary64);
        forms { 0xDD / 0 => operands(mem); }
    }
    FLD_EXTENDED {
        execute: load_extended;
        forms { 0xDB / 5 => operands(mem); }
    }
    FILD {
        execute: load_integer::<_>;
        forms {
            0xDF / 0 => word(mem);
            0xDB / 0 => dword(mem);
            0xDF / 5 => qword(mem);
        }
    }
    FLD_REGISTER {
        execute: load_register;
        forms { 0xD9 @ 0xC0 + rm => operands(st); }
    }
}

fn load_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, format.bytes(), Intent::Read, &[])?;
    let source = operand.read_x87_binary(execution, format)?;
    execution.specialize(|jit| {
        let available = jit.x87().push_available()?;
        jit.specialize_on(
            source
                .signaling_nan
                .eq(false)
                .and(source.denormal.eq(false))
                .and(available),
        )
    })?;
    execution.record_x87_memory(&operand)?;
    execution.x87().push(LoadSource::Binary(source))
}

fn load_extended(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, 10, Intent::Read, &[])?;
    let value = ExtendedValue::from_bits(ExtendedBits {
        significand: operand.read::<I64>(execution, 0)?,
        sign_exponent: operand.read::<I16>(execution, 8)?,
    });
    execution.record_x87_memory(&operand)?;
    execution.x87().push(LoadSource::Value(value))
}

fn load_integer<T: MemoryInt>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, T::BYTES, Intent::Read, &[])?;
    let integer = operand.read::<T>(execution, 0)?;
    execution.specialize(|jit| {
        let available = jit.x87().push_available()?;
        jit.specialize_on(available)
    })?;
    let value = ExtendedValue::from_signed_integer(&integer);
    execution.record_x87_memory(&operand)?;
    execution.x87().push(LoadSource::Value(value))
}

fn load_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    // The source uses the old TOP, including when the push destination aliases it.
    let source = execution.x87().read_stack(source.offset())?;
    execution.x87().push(LoadSource::Register(source))
}

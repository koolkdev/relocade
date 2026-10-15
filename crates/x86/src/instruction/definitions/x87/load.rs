//! x87 loads push converted memory values, register copies, or constants.

use super::*;
use crate::{
    address::MemoryAddress,
    floating::BinaryFormat,
    instruction::X87StackIndex,
    memory::Intent,
    state::x87::{ConditionCode, Exception, StackValue},
    x87::{BinaryOperand, Constant, ExtendedBits, ExtendedValue, PackedBcd},
};
use wasm86_compiler::{MemoryInt, I64};

instruction_families! {
    FLD1 {
        execute: load_constant(Constant::One);
        requires: [Fpu];
        forms { 0xD9 @ 0xE8 => no_operands(); }
    }
    FLDL2T {
        execute: load_constant(Constant::Log2Ten);
        requires: [Fpu];
        forms { 0xD9 @ 0xE9 => no_operands(); }
    }
    FLDL2E {
        execute: load_constant(Constant::Log2E);
        requires: [Fpu];
        forms { 0xD9 @ 0xEA => no_operands(); }
    }
    FLDPI {
        execute: load_constant(Constant::Pi);
        requires: [Fpu];
        forms { 0xD9 @ 0xEB => no_operands(); }
    }
    FLDLG2 {
        execute: load_constant(Constant::Log10Two);
        requires: [Fpu];
        forms { 0xD9 @ 0xEC => no_operands(); }
    }
    FLDLN2 {
        execute: load_constant(Constant::LnTwo);
        requires: [Fpu];
        forms { 0xD9 @ 0xED => no_operands(); }
    }
    FLDZ {
        execute: load_constant(Constant::Zero);
        requires: [Fpu];
        forms { 0xD9 @ 0xEE => no_operands(); }
    }
    FLD_BINARY32 {
        execute: load_binary(BinaryFormat::Binary32);
        requires: [Fpu];
        forms { 0xD9 / 0 => operands(mem); }
    }
    FLD_BINARY64 {
        execute: load_binary(BinaryFormat::Binary64);
        requires: [Fpu];
        forms { 0xDD / 0 => operands(mem); }
    }
    FLD_EXTENDED {
        execute: load_extended;
        requires: [Fpu];
        forms { 0xDB / 5 => operands(mem); }
    }
    FILD {
        execute: load_integer::<_>;
        requires: [Fpu];
        forms {
            0xDF / 0 => word(mem);
            0xDB / 0 => dword(mem);
            0xDF / 5 => qword(mem);
        }
    }
    FBLD {
        execute: load_bcd;
        requires: [Fpu];
        forms { 0xDF / 4 => operands(mem); }
    }
    FLD_REGISTER {
        execute: load_register;
        requires: [Fpu];
        forms { 0xD9 @ 0xC0 + rm => operands(st); }
    }
}

fn load_constant(
    execution: &mut ExecutionBuilder<'_, '_>,
    constant: Constant,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.specialize_x87_push(true)?;
    let rounding = execution.x87().rounding()?;
    let value = constant.value(&rounding);
    execution.record_x87_instruction()?;
    push(execution, LoadSource::Value(value))
}

fn load_binary(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, format.bytes(), Intent::Read, &[])?;
    let source = operand.read_x87_binary(execution, format)?;
    execution.specialize_x87_push(
        source
            .signaling_nan
            .eq(false)
            .and(source.denormal.eq(false)),
    )?;
    execution.record_x87_memory(&operand)?;
    push(execution, LoadSource::Binary(source))
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
    push(execution, LoadSource::Value(value))
}

fn load_integer<T: MemoryInt + crate::memory::TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, T::BYTES, Intent::Read, &[])?;
    let integer = operand.read::<T>(execution, 0)?;
    execution.specialize_x87_push(true)?;
    let value = ExtendedValue::from_signed_integer(&integer);
    execution.record_x87_memory(&operand)?;
    push(execution, LoadSource::Value(value))
}

fn load_bcd(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let operand = execution.memory_operand(address, 10, Intent::Read, &[])?;
    let bcd = PackedBcd {
        low: operand.read::<I64>(execution, 0)?,
        high: operand.read::<I16>(execution, 8)?,
    };
    execution.specialize_x87_push(true)?;
    execution.record_x87_memory(&operand)?;
    push(execution, LoadSource::Value(bcd.decode()))
}

fn load_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    // The source uses the old TOP, including when the push destination aliases it.
    let source = execution.x87().read_stack(source.offset())?;
    push(execution, LoadSource::Register(source))
}

/// Load provenance determines operand exceptions independently of stack faults.
/// Extended transfers and exact integer conversions supply a value directly;
/// only narrow real loads classify SNaNs and denormals as operands.
enum LoadSource {
    Value(ExtendedValue),
    Register(StackValue),
    Binary(BinaryOperand),
}

fn push(execution: &mut ExecutionBuilder<'_, '_>, source: LoadSource) -> Result<(), BuildError> {
    let mut x87 = execution.x87();
    let (value, source_empty, signaling_nan, denormal) = match source {
        LoadSource::Value(value) => (value, false.into(), false.into(), false.into()),
        LoadSource::Register(source) => {
            let empty = source.is_empty();
            (source.value, empty, false.into(), false.into())
        }
        LoadSource::Binary(source) => (
            source.loaded_value(),
            false.into(),
            source.signaling_nan,
            source.denormal,
        ),
    };
    let full = x87.push_full()?;
    let fault = source_empty.or(&full);
    // A missing source takes priority over an occupied push destination.
    let overflow = source_empty.eq(false).and(full);
    // A stack fault suppresses source conversion exceptions even when the
    // invalid-operation exception is masked.
    let invalid = fault.or(signaling_nan);
    let denormal = invalid.eq(false).and(denormal);
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &invalid)?;
    let unmasked_denormal = x87.record_exception(Exception::Denormal, &denormal)?;
    x87.record_stack_fault(&fault)?;
    x87.record_pending_exception(unmasked_invalid.or(unmasked_denormal))?;
    x87.set_condition_code(ConditionCode::C1, overflow)?;
    // FLD completes a denormal load even with DM clear (Intel Vol. 2,
    // FLD description). Only an unmasked invalid exception suppresses it.
    let enabled = unmasked_invalid.eq(false);
    let value = value.or_indefinite(&fault);
    x87.push(&value, &enabled)
}

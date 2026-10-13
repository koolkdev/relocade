//! Comparisons publish x87 condition codes and optionally pop one or two slots.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::{X87Operand, X87Operands},
    instruction::X87StackIndex,
    state::x87::{ConditionCode, Exception},
    x87::{BinaryFormat, ComparisonKind, ExtendedValue},
};

instruction_families! {
    FCOM_REGISTER {
        execute: compare_register(ComparisonKind::Ordered, 0);
        forms { 0xD8 @ 0xD0 + rm => operands(st); }
    }
    FCOMP_REGISTER {
        execute: compare_register(ComparisonKind::Ordered, 1);
        forms { 0xD8 @ 0xD8 + rm => operands(st); }
    }
    FCOMPP {
        execute: compare_register(X87StackIndex::new(1), ComparisonKind::Ordered, 2);
        forms { 0xDE @ 0xD9 => no_operands(); }
    }
    FUCOM {
        execute: compare_register(ComparisonKind::Unordered, 0);
        forms { 0xDD @ 0xE0 + rm => operands(st); }
    }
    FUCOMP {
        execute: compare_register(ComparisonKind::Unordered, 1);
        forms { 0xDD @ 0xE8 + rm => operands(st); }
    }
    FUCOMPP {
        execute: compare_register(X87StackIndex::new(1), ComparisonKind::Unordered, 2);
        forms { 0xDA @ 0xE9 => no_operands(); }
    }
    FCOM_BINARY32 {
        execute: compare_memory(BinaryFormat::Binary32, 0);
        forms { 0xD8 / 2 => operands(mem); }
    }
    FCOM_BINARY64 {
        execute: compare_memory(BinaryFormat::Binary64, 0);
        forms { 0xDC / 2 => operands(mem); }
    }
    FCOMP_BINARY32 {
        execute: compare_memory(BinaryFormat::Binary32, 1);
        forms { 0xD8 / 3 => operands(mem); }
    }
    FCOMP_BINARY64 {
        execute: compare_memory(BinaryFormat::Binary64, 1);
        forms { 0xDC / 3 => operands(mem); }
    }
    FTST {
        execute: test_zero;
        forms { 0xD9 @ 0xE4 => no_operands(); }
    }
}

fn compare_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
    kind: ComparisonKind,
    pop_count: u32,
) -> Result<(), BuildError> {
    compare(
        execution,
        X87Operand::Register(source.offset()),
        kind,
        pop_count,
    )
}

fn compare_memory(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
    pop_count: u32,
) -> Result<(), BuildError> {
    compare(
        execution,
        X87Operand::BinaryMemory { address, format },
        ComparisonKind::Ordered,
        pop_count,
    )
}

fn test_zero(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let zero = ExtendedValue::from_signed_integer(&Val::<I32>::from(0));
    compare(
        execution,
        X87Operand::Value(zero),
        ComparisonKind::Ordered,
        0,
    )
}

/// Ordinary comparisons, including FUCOM quiet NaNs, stay in the block.
/// Operand exceptions restart before metadata, condition codes or pops.
fn compare(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87Operand,
    kind: ComparisonKind,
    pop_count: u32,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let X87Operands {
        values,
        mut stack_fault,
        memory,
    } = execution.read_x87_operands(0.into(), source)?;
    let mut result = values.compare(kind);
    execution.specialize(|execution| {
        execution.specialize_on(
            stack_fault
                .or(&result.invalid)
                .or(&result.denormal)
                .eq(false),
        )?;
        stack_fault = false.into();
        result.invalid = false.into();
        result.denormal = false.into();
        Ok(())
    })?;
    execution.record_x87_operand(memory.as_ref())?;

    // Empty stack entries override numerical responses, including a
    // denormal memory source that expanded to a normal extended value.
    result.invalid = stack_fault.or(result.invalid);
    result.unordered = stack_fault.or(result.unordered);
    result.denormal = stack_fault.eq(false).and(result.denormal);
    let mut x87 = execution.x87();
    let unmasked_invalid = x87.record_exception(Exception::Invalid, &result.invalid)?;
    let unmasked_denormal = x87.record_exception(Exception::Denormal, &result.denormal)?;
    x87.record_stack_fault(&stack_fault)?;
    let suppressed = unmasked_invalid.or(unmasked_denormal);
    x87.record_pending_exception(suppressed.clone())?;
    x87.set_condition_code(ConditionCode::C1, false)?;
    // Unmasked invalid preserves the condition codes. Retain them for an
    // unmasked denormal too, as the pre-operation policy; Intel specifies
    // unchanged TOP and operands for #D but not the condition-code response.
    let enabled = suppressed.eq(false);
    x87.set_condition_code_when(
        ConditionCode::C0,
        result.unordered.or(&result.less),
        &enabled,
    )?;
    x87.set_condition_code_when(ConditionCode::C2, result.unordered.clone(), &enabled)?;
    x87.set_condition_code_when(
        ConditionCode::C3,
        result.unordered.or(&result.equal),
        &enabled,
    )?;
    x87.pop_n(pop_count, &enabled)
}

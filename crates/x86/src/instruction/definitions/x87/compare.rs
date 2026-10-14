//! Comparisons select an architectural result destination and optional stack pops.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::{X87MemoryFormat, X87Operand, X87Operands},
    flags::{Flag, FlagChange},
    floating::BinaryFormat,
    instruction::X87StackIndex,
    state::x87::{ConditionCode, Exception},
    x87::{ComparisonKind, ExtendedValue},
};

instruction_families! {
    FCOM_REGISTER {
        execute: compare_register(ComparisonKind::Ordered, 0, ComparisonTarget::X87);
        forms { 0xD8 @ 0xD0 + rm => operands(st); }
    }
    FCOMP_REGISTER {
        execute: compare_register(ComparisonKind::Ordered, 1, ComparisonTarget::X87);
        forms { 0xD8 @ 0xD8 + rm => operands(st); }
    }
    FCOMPP {
        execute: compare_register(X87StackIndex::new(1), ComparisonKind::Ordered, 2, ComparisonTarget::X87);
        forms { 0xDE @ 0xD9 => no_operands(); }
    }
    FUCOM {
        execute: compare_register(ComparisonKind::Unordered, 0, ComparisonTarget::X87);
        forms { 0xDD @ 0xE0 + rm => operands(st); }
    }
    FUCOMP {
        execute: compare_register(ComparisonKind::Unordered, 1, ComparisonTarget::X87);
        forms { 0xDD @ 0xE8 + rm => operands(st); }
    }
    FUCOMPP {
        execute: compare_register(X87StackIndex::new(1), ComparisonKind::Unordered, 2, ComparisonTarget::X87);
        forms { 0xDA @ 0xE9 => no_operands(); }
    }
    FCOMI {
        execute: compare_register(ComparisonKind::Ordered, 0, ComparisonTarget::Eflags);
        forms { 0xDB @ 0xF0 + rm => operands(st); }
    }
    FCOMIP {
        execute: compare_register(ComparisonKind::Ordered, 1, ComparisonTarget::Eflags);
        forms { 0xDF @ 0xF0 + rm => operands(st); }
    }
    FUCOMI {
        execute: compare_register(ComparisonKind::Unordered, 0, ComparisonTarget::Eflags);
        forms { 0xDB @ 0xE8 + rm => operands(st); }
    }
    FUCOMIP {
        execute: compare_register(ComparisonKind::Unordered, 1, ComparisonTarget::Eflags);
        forms { 0xDF @ 0xE8 + rm => operands(st); }
    }
    FCOM_BINARY32 {
        execute: compare_memory(X87MemoryFormat::Binary(BinaryFormat::Binary32), 0);
        forms { 0xD8 / 2 => operands(mem); }
    }
    FCOM_BINARY64 {
        execute: compare_memory(X87MemoryFormat::Binary(BinaryFormat::Binary64), 0);
        forms { 0xDC / 2 => operands(mem); }
    }
    FCOMP_BINARY32 {
        execute: compare_memory(X87MemoryFormat::Binary(BinaryFormat::Binary32), 1);
        forms { 0xD8 / 3 => operands(mem); }
    }
    FCOMP_BINARY64 {
        execute: compare_memory(X87MemoryFormat::Binary(BinaryFormat::Binary64), 1);
        forms { 0xDC / 3 => operands(mem); }
    }
    FICOM_INTEGER16 {
        execute: compare_memory(X87MemoryFormat::Integer16, 0);
        forms { 0xDE / 2 => operands(mem); }
    }
    FICOM_INTEGER32 {
        execute: compare_memory(X87MemoryFormat::Integer32, 0);
        forms { 0xDA / 2 => operands(mem); }
    }
    FICOMP_INTEGER16 {
        execute: compare_memory(X87MemoryFormat::Integer16, 1);
        forms { 0xDE / 3 => operands(mem); }
    }
    FICOMP_INTEGER32 {
        execute: compare_memory(X87MemoryFormat::Integer32, 1);
        forms { 0xDA / 3 => operands(mem); }
    }
    FTST {
        execute: test_zero;
        forms { 0xD9 @ 0xE4 => no_operands(); }
    }
}

#[derive(Clone, Copy)]
enum ComparisonTarget {
    X87,
    Eflags,
}

fn compare_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
    kind: ComparisonKind,
    pop_count: u32,
    target: ComparisonTarget,
) -> Result<(), BuildError> {
    compare(
        execution,
        X87Operand::Register(source.offset()),
        kind,
        pop_count,
        target,
    )
}

fn compare_memory(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: X87MemoryFormat,
    pop_count: u32,
) -> Result<(), BuildError> {
    compare(
        execution,
        X87Operand::Memory { address, format },
        ComparisonKind::Ordered,
        pop_count,
        ComparisonTarget::X87,
    )
}

fn test_zero(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let zero = ExtendedValue::from_signed_integer(&Val::<I32>::from(0));
    compare(
        execution,
        X87Operand::Value(zero),
        ComparisonKind::Ordered,
        0,
        ComparisonTarget::X87,
    )
}

/// Ordinary comparisons, including FUCOM quiet NaNs, stay in the block.
/// Operand exceptions restart before metadata, condition codes or pops.
fn compare(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87Operand,
    kind: ComparisonKind,
    pop_count: u32,
    target: ComparisonTarget,
) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let X87Operands {
        values,
        mut stack_fault,
        memory,
    } = execution.read_x87_operands(0.into(), source)?;
    let mut result = values.compare(kind);
    execution.specialize_without([&mut stack_fault, &mut result.invalid, &mut result.denormal])?;
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
    let pop_enabled = suppressed.eq(false);
    if matches!(target, ComparisonTarget::X87) {
        // Unmasked invalid preserves the x87 condition codes. Retain them
        // for unmasked denormals too; Intel specifies unchanged TOP and
        // operands for #D but not the condition-code response.
        x87.set_condition_code_when(
            ConditionCode::C0,
            result.unordered.or(&result.less),
            &pop_enabled,
        )?;
        x87.set_condition_code_when(ConditionCode::C2, result.unordered.clone(), &pop_enabled)?;
        x87.set_condition_code_when(
            ConditionCode::C3,
            result.unordered.or(&result.equal),
            &pop_enabled,
        )?;
    }
    x87.pop_n(pop_count, &pop_enabled)?;
    if matches!(target, ComparisonTarget::Eflags) {
        // Intel's Celeron specification update 243748-011, C15, corrects
        // the manual: comparison flags are written even for unmasked #IA.
        // Denormals also publish the relation; neither exception allows a
        // pop when unmasked. OF/SF/AF clear per 252046-033, change 8.
        execution.write_flags(FlagChange::partial([
            (Flag::CF, result.unordered.or(result.less)),
            (Flag::PF, result.unordered.clone()),
            (Flag::ZF, result.unordered.or(result.equal)),
            (Flag::OF, false.into()),
            (Flag::SF, false.into()),
            (Flag::AF, false.into()),
        ]))?;
    }
    Ok(())
}

//! Comparisons select an architectural result destination and optional stack pops.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::{ComparisonTarget, X87MemoryFormat, X87Operand},
    instruction::X87StackIndex,
    x87::{BinaryFormat, ComparisonKind, ExtendedValue},
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

fn compare_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
    kind: ComparisonKind,
    pops: u32,
    target: ComparisonTarget,
) -> Result<(), BuildError> {
    execution.compare_x87(X87Operand::Register(source.offset()), kind, pops, target)
}

fn compare_memory(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: X87MemoryFormat,
    pops: u32,
) -> Result<(), BuildError> {
    execution.compare_x87(
        X87Operand::Memory { address, format },
        ComparisonKind::Ordered,
        pops,
        ComparisonTarget::X87,
    )
}

fn test_zero(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let zero = ExtendedValue::from_signed_integer(&Val::<I32>::from(0));
    execution.compare_x87(
        X87Operand::Value(zero),
        ComparisonKind::Ordered,
        0,
        ComparisonTarget::X87,
    )
}

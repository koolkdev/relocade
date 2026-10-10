//! Comparisons publish x87 condition codes and optionally pop one or two slots.

use super::*;
use crate::{
    address::MemoryAddress,
    execution::X87Operand,
    instruction::X87StackIndex,
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
    pops: u32,
) -> Result<(), BuildError> {
    execution.compare_x87(X87Operand::Register(source.offset()), kind, pops)
}

fn compare_memory(
    execution: &mut ExecutionBuilder<'_, '_>,
    address: MemoryAddress<Val<I32>>,
    format: BinaryFormat,
    pops: u32,
) -> Result<(), BuildError> {
    execution.compare_x87(
        X87Operand::BinaryMemory { address, format },
        ComparisonKind::Ordered,
        pops,
    )
}

fn test_zero(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let zero = ExtendedValue::from_signed_integer(&Val::<I32>::from(0));
    execution.compare_x87(X87Operand::Value(zero), ComparisonKind::Ordered, 0)
}

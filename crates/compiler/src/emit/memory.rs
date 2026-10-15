//! Select memory instructions after all address and value operands are scheduled.
use wasm_encoder::{Instruction, MemArg};

use crate::{
    memory::{AtomicKind, AtomicUpdate, MemoryAccess},
    Type,
};

pub(super) fn atomic(argument: MemArg, bytes: u8, operation: AtomicKind) -> Instruction<'static> {
    use AtomicKind::*;
    use AtomicUpdate::*;
    match (operation, bytes) {
        (Load, 1) => Instruction::I32AtomicLoad8U(argument),
        (Load, 2) => Instruction::I32AtomicLoad16U(argument),
        (Load, 4) => Instruction::I32AtomicLoad(argument),
        (Load, 8) => Instruction::I64AtomicLoad(argument),
        (Store, 1) => Instruction::I32AtomicStore8(argument),
        (Store, 2) => Instruction::I32AtomicStore16(argument),
        (Store, 4) => Instruction::I32AtomicStore(argument),
        (Store, 8) => Instruction::I64AtomicStore(argument),
        (CompareExchange, 1) => Instruction::I32AtomicRmw8CmpxchgU(argument),
        (CompareExchange, 2) => Instruction::I32AtomicRmw16CmpxchgU(argument),
        (CompareExchange, 4) => Instruction::I32AtomicRmwCmpxchg(argument),
        (CompareExchange, 8) => Instruction::I64AtomicRmwCmpxchg(argument),
        (Update(Add), 1) => Instruction::I32AtomicRmw8AddU(argument),
        (Update(Add), 2) => Instruction::I32AtomicRmw16AddU(argument),
        (Update(Add), 4) => Instruction::I32AtomicRmwAdd(argument),
        (Update(Add), 8) => Instruction::I64AtomicRmwAdd(argument),
        (Update(Subtract), 1) => Instruction::I32AtomicRmw8SubU(argument),
        (Update(Subtract), 2) => Instruction::I32AtomicRmw16SubU(argument),
        (Update(Subtract), 4) => Instruction::I32AtomicRmwSub(argument),
        (Update(Subtract), 8) => Instruction::I64AtomicRmwSub(argument),
        (Update(And), 1) => Instruction::I32AtomicRmw8AndU(argument),
        (Update(And), 2) => Instruction::I32AtomicRmw16AndU(argument),
        (Update(And), 4) => Instruction::I32AtomicRmwAnd(argument),
        (Update(And), 8) => Instruction::I64AtomicRmwAnd(argument),
        (Update(Or), 1) => Instruction::I32AtomicRmw8OrU(argument),
        (Update(Or), 2) => Instruction::I32AtomicRmw16OrU(argument),
        (Update(Or), 4) => Instruction::I32AtomicRmwOr(argument),
        (Update(Or), 8) => Instruction::I64AtomicRmwOr(argument),
        (Update(Xor), 1) => Instruction::I32AtomicRmw8XorU(argument),
        (Update(Xor), 2) => Instruction::I32AtomicRmw16XorU(argument),
        (Update(Xor), 4) => Instruction::I32AtomicRmwXor(argument),
        (Update(Xor), 8) => Instruction::I64AtomicRmwXor(argument),
        (Update(Exchange), 1) => Instruction::I32AtomicRmw8XchgU(argument),
        (Update(Exchange), 2) => Instruction::I32AtomicRmw16XchgU(argument),
        (Update(Exchange), 4) => Instruction::I32AtomicRmwXchg(argument),
        (Update(Exchange), 8) => Instruction::I64AtomicRmwXchg(argument),
        _ => unreachable!("ordered memory accesses retain their logical width"),
    }
}

pub(super) fn argument(memories: &[Option<u32>], access: MemoryAccess) -> MemArg {
    MemArg {
        offset: u64::from(access.offset),
        align: access.bytes.trailing_zeros(),
        memory_index: memories[access.memory.0].expect("an authored memory is imported"),
    }
}

#[derive(Clone, Copy)]
pub(super) enum StoreForm {
    Plain,
    Lane { lane: u8 },
}

pub(super) fn store(
    argument: MemArg,
    bytes: u8,
    value_type: Type,
    form: StoreForm,
) -> Instruction<'static> {
    match (form, value_type.carrier(), bytes) {
        (StoreForm::Plain, Type::I32, 1) => Instruction::I32Store8(argument),
        (StoreForm::Plain, Type::I32, 2) => Instruction::I32Store16(argument),
        (StoreForm::Plain, Type::I32, 4) => Instruction::I32Store(argument),
        (StoreForm::Plain, Type::I64, 1) => Instruction::I64Store8(argument),
        (StoreForm::Plain, Type::I64, 2) => Instruction::I64Store16(argument),
        (StoreForm::Plain, Type::I64, 4) => Instruction::I64Store32(argument),
        (StoreForm::Plain, Type::I64, 8) => Instruction::I64Store(argument),
        (StoreForm::Plain, Type::F64, 8) => Instruction::F64Store(argument),
        (StoreForm::Plain, Type::V128, 8) => Instruction::V128Store64Lane {
            memarg: argument,
            lane: 0,
        },
        (StoreForm::Plain, Type::V128, 16) => Instruction::V128Store(argument),
        (StoreForm::Lane { lane }, Type::V128, 1) => Instruction::V128Store8Lane {
            memarg: argument,
            lane,
        },
        (StoreForm::Lane { lane }, Type::V128, 2) => Instruction::V128Store16Lane {
            memarg: argument,
            lane,
        },
        (StoreForm::Lane { lane }, Type::V128, 4) => Instruction::V128Store32Lane {
            memarg: argument,
            lane,
        },
        (StoreForm::Lane { lane }, Type::V128, 8) => Instruction::V128Store64Lane {
            memarg: argument,
            lane,
        },
        _ => unreachable!("a store retains its carrier and storage width"),
    }
}

#[derive(Clone, Copy)]
pub(super) enum LoadForm {
    Plain,
    Signed,
    Splat64,
}

pub(super) fn load(
    argument: MemArg,
    bytes: u8,
    target: Type,
    form: LoadForm,
) -> Instruction<'static> {
    match (form, target.carrier(), bytes) {
        (LoadForm::Plain, Type::I32, 1) => Instruction::I32Load8U(argument),
        (LoadForm::Signed, Type::I32, 1) => Instruction::I32Load8S(argument),
        (LoadForm::Plain, Type::I32, 2) => Instruction::I32Load16U(argument),
        (LoadForm::Signed, Type::I32, 2) => Instruction::I32Load16S(argument),
        (LoadForm::Plain, Type::I32, 4) => Instruction::I32Load(argument),
        (LoadForm::Signed, Type::I64, 1) => Instruction::I64Load8S(argument),
        (LoadForm::Signed, Type::I64, 2) => Instruction::I64Load16S(argument),
        (LoadForm::Signed, Type::I64, 4) => Instruction::I64Load32S(argument),
        (LoadForm::Plain, Type::I64, 8) => Instruction::I64Load(argument),
        (LoadForm::Plain, Type::F64, 8) => Instruction::F64Load(argument),
        (LoadForm::Plain, Type::V128, 8) => Instruction::V128Load64Zero(argument),
        (LoadForm::Plain, Type::V128, 16) => Instruction::V128Load(argument),
        (LoadForm::Splat64, Type::V128, 8) => Instruction::V128Load64Splat(argument),
        _ => unreachable!("a load retains its type or widens with its original sign"),
    }
}

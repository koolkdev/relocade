//! Memory instruction selection after read placement preserves authored snapshots.
use wasm_encoder::{Encode, Instruction, MemArg};

use super::Scheduler;
use crate::{
    memory::{AtomicKind, AtomicOperation, Location},
    place, Type, ValueKind,
};

impl Scheduler<'_> {
    pub(super) fn atomic_operation(&mut self, access: &AtomicOperation) {
        use AtomicKind::*;
        let argument = self.memory_argument(access.location);
        let instruction = match (access.operation, access.location.bytes) {
            (Load, 1) => Instruction::I32AtomicLoad8U(argument),
            (Load, 2) => Instruction::I32AtomicLoad16U(argument),
            (Load, 4) => Instruction::I32AtomicLoad(argument),
            (Load, 8) => Instruction::I64AtomicLoad(argument),
            (Store { .. }, 1) => Instruction::I32AtomicStore8(argument),
            (Store { .. }, 2) => Instruction::I32AtomicStore16(argument),
            (Store { .. }, 4) => Instruction::I32AtomicStore(argument),
            (Store { .. }, 8) => Instruction::I64AtomicStore(argument),
            (CompareExchange { .. }, 1) => Instruction::I32AtomicRmw8CmpxchgU(argument),
            (CompareExchange { .. }, 2) => Instruction::I32AtomicRmw16CmpxchgU(argument),
            (CompareExchange { .. }, 4) => Instruction::I32AtomicRmwCmpxchg(argument),
            (CompareExchange { .. }, 8) => Instruction::I64AtomicRmwCmpxchg(argument),
            (Add(_), 1) => Instruction::I32AtomicRmw8AddU(argument),
            (Add(_), 2) => Instruction::I32AtomicRmw16AddU(argument),
            (Add(_), 4) => Instruction::I32AtomicRmwAdd(argument),
            (Add(_), 8) => Instruction::I64AtomicRmwAdd(argument),
            (Subtract(_), 1) => Instruction::I32AtomicRmw8SubU(argument),
            (Subtract(_), 2) => Instruction::I32AtomicRmw16SubU(argument),
            (Subtract(_), 4) => Instruction::I32AtomicRmwSub(argument),
            (Subtract(_), 8) => Instruction::I64AtomicRmwSub(argument),
            (And(_), 1) => Instruction::I32AtomicRmw8AndU(argument),
            (And(_), 2) => Instruction::I32AtomicRmw16AndU(argument),
            (And(_), 4) => Instruction::I32AtomicRmwAnd(argument),
            (And(_), 8) => Instruction::I64AtomicRmwAnd(argument),
            (Or(_), 1) => Instruction::I32AtomicRmw8OrU(argument),
            (Or(_), 2) => Instruction::I32AtomicRmw16OrU(argument),
            (Or(_), 4) => Instruction::I32AtomicRmwOr(argument),
            (Or(_), 8) => Instruction::I64AtomicRmwOr(argument),
            (Xor(_), 1) => Instruction::I32AtomicRmw8XorU(argument),
            (Xor(_), 2) => Instruction::I32AtomicRmw16XorU(argument),
            (Xor(_), 4) => Instruction::I32AtomicRmwXor(argument),
            (Xor(_), 8) => Instruction::I64AtomicRmwXor(argument),
            (Exchange(_), 1) => Instruction::I32AtomicRmw8XchgU(argument),
            (Exchange(_), 2) => Instruction::I32AtomicRmw16XchgU(argument),
            (Exchange(_), 4) => Instruction::I32AtomicRmwXchg(argument),
            (Exchange(_), 8) => Instruction::I64AtomicRmwXchg(argument),
            _ => unreachable!("ordered memory accesses retain their logical width"),
        };
        instruction.encode(&mut self.bytes);
    }

    pub(super) fn signed_load_location(&self, mut input: usize) -> Option<Location> {
        let mut bits = self.body.values[input].ty.bits();
        loop {
            input = place::representation(self.body, input);
            // Saved reads and signed values retain their sharing and snapshots.
            if self.placement.slots[input].is_some() {
                return None;
            }
            match self.body.values[input].kind {
                ValueKind::SignExtend(original) if self.body.values[original].ty.bits() <= bits => {
                    // An unshared extension can be covered with the wider one.
                    // Narrowing below its original sign would change the value.
                    bits = self.body.values[original].ty.bits();
                    input = original;
                }
                ValueKind::Load { location, .. } => {
                    // Cover the full original read; conversions must not change
                    // its access width or choose a different logical sign bit.
                    return (bits == location.bytes * 8).then_some(location);
                }
                _ => return None,
            }
        }
    }

    pub(super) fn memory_argument(&self, location: Location) -> MemArg {
        MemArg {
            offset: u64::from(location.offset),
            align: location.bytes.trailing_zeros(),
            memory_index: self.memories[location.memory.0].expect("an authored memory is imported"),
        }
    }

    pub(super) fn load(&mut self, location: Location, target: Type, signed: bool) {
        let argument = self.memory_argument(location);
        match (target == Type::I64, location.bytes, signed) {
            (false, 1, false) => Instruction::I32Load8U(argument),
            (false, 1, true) => Instruction::I32Load8S(argument),
            (false, 2, false) => Instruction::I32Load16U(argument),
            (false, 2, true) => Instruction::I32Load16S(argument),
            (false, 4, false) => Instruction::I32Load(argument),
            (true, 1, true) => Instruction::I64Load8S(argument),
            (true, 2, true) => Instruction::I64Load16S(argument),
            (true, 4, true) => Instruction::I64Load32S(argument),
            (true, 8, false) => Instruction::I64Load(argument),
            _ => unreachable!("a load retains its type or widens with its original sign"),
        }
        .encode(&mut self.bytes);
    }
}

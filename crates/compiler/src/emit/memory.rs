//! Memory instruction selection after read placement preserves authored snapshots.
use wasm_encoder::{Encode, Instruction, MemArg};

use super::Scheduler;
use crate::{memory::Location, place, Type, ValueKind};

impl Scheduler<'_> {
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

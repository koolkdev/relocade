//! Transfers between AH and the low architectural flag byte.

use super::*;
use crate::flags::{Flag, FlagChange};
use wasm86_compiler::I1;

instruction_families! {
    LAHF {
        execute: load_flags;
        forms {
            0x9F => byte(AH);
        }
    }
    SAHF {
        execute: store_flags;
        forms {
            0x9E => byte(AH);
        }
    }
}

const STATUS_BITS: [(Flag, u32); 5] = [
    (Flag::CF, 0),
    (Flag::PF, 2),
    (Flag::AF, 4),
    (Flag::ZF, 6),
    (Flag::SF, 7),
];

fn load_flags(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I8>,
) -> Result<(), BuildError> {
    let values = execution.read_flags(STATUS_BITS.map(|(flag, _)| flag))?;
    let mut byte: Val<I8> = 0x02.into();
    for ((_, bit), value) in STATUS_BITS.into_iter().zip(values) {
        byte = byte.or(value.unsigned().extend::<I8>().shl(bit));
    }
    destination.write(execution, byte)
}

fn store_flags(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Input<I8>,
) -> Result<(), BuildError> {
    let byte = source.read(execution)?;
    execution.write_flags(FlagChange::partial(
        STATUS_BITS.map(|(flag, bit)| (flag, byte.unsigned().shr(bit).truncate::<I1>())),
    ))
}

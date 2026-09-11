//! First or last set-bit positions and their status flags.

use wasm86_compiler::{MemoryInt, Val};

use super::{
    flags::{AnyFlagSource, FlagChange, FlagSource},
    AluResult,
};

#[derive(Clone, Copy)]
pub(crate) enum BitScanOp {
    Forward,
    Reverse,
}

impl BitScanOp {
    pub(crate) fn apply<T: MemoryInt>(self, source: Val<T>, previous: Val<T>) -> AluResult<T>
    where
        FlagSource<T>: Into<AnyFlagSource>,
    {
        let zero = source.eq(0);
        let index = match self {
            Self::Forward => source.ctz(),
            Self::Reverse => Val::<T>::from(T::BYTES * 8 - 1).sub(source.clz()),
        };
        // Scan parity covers the full logical source, unlike ordinary ALU
        // result parity, which observes only the low byte.
        let flags = FlagSource::<T>::Explicit {
            flags: [
                false.into(),
                source.popcnt().and(1).eq(0),
                false.into(),
                zero.clone(),
                false.into(),
                false.into(),
            ],
        };
        AluResult {
            result: zero.select(previous, index),
            flags: FlagChange::from(flags),
        }
    }
}

//! First or last set-bit positions and their status flags.

use wasm86_compiler::{MemoryInt, Val};

use super::AluResult;
use crate::alu::{AnyStatusSource, StatusSource};
use crate::flags::FlagChange;

#[derive(Clone, Copy)]
pub(crate) enum BitScanOp {
    Forward,
    Reverse,
}

impl BitScanOp {
    pub(crate) fn apply<T: MemoryInt>(self, source: Val<T>, previous: Val<T>) -> AluResult<T>
    where
        StatusSource<T>: Into<AnyStatusSource>,
    {
        let zero = source.eq(0);
        let index = match self {
            Self::Forward => source.ctz(),
            Self::Reverse => Val::<T>::from(T::BYTES * 8 - 1).sub(source.clz()),
        };
        // Undefined-result policy: preserve the destination on zero, clear
        // CF/AF/SF/OF, and compute PF over the full logical source. ZF is the only
        // architecturally defined flag. Ordinary ALU parity uses the low byte.
        let flags = StatusSource::<T>::Explicit {
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

//! Full integer products and signed or unsigned overflow at the operand width.

use wasm86_compiler::Val;

use super::{AluResult, DoubleWidth};
use crate::alu::{AnyStatusSource, StatusSource};
use crate::flags::FlagChange;

#[derive(Clone, Copy)]
pub(crate) enum MultiplyOp {
    Signed,
    Unsigned,
}

impl MultiplyOp {
    pub(crate) fn apply<T: DoubleWidth>(self, left: Val<T>, right: Val<T>) -> AluResult<T::Double>
    where
        StatusSource<T>: Into<AnyStatusSource>,
    {
        let result = match self {
            Self::Signed => left
                .signed()
                .extend::<T::Double>()
                .mul(right.signed().extend::<T::Double>()),
            Self::Unsigned => left
                .unsigned()
                .extend::<T::Double>()
                .mul(right.unsigned().extend::<T::Double>()),
        };
        let overflow = match self {
            Self::Signed => result.ne(result.truncate::<T>().signed().extend::<T::Double>()),
            Self::Unsigned => result.unsigned().shr(T::BYTES * 8).ne(0),
        };
        // PF/AF/ZF/SF are undefined. Choose 1/0/0/0 without reading old flags.
        let flags = StatusSource::<T>::Explicit {
            flags: [
                overflow.clone(),
                true.into(),
                false.into(),
                false.into(),
                false.into(),
                overflow,
            ],
        };
        AluResult {
            result,
            flags: FlagChange::from(flags),
        }
    }
}

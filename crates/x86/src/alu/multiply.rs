//! Full integer products and signed or unsigned overflow at the operand width.

use wasm86_compiler::{AtLeast, MemoryInt, Val, I16, I32, I64, I8};

use super::{
    flags::{AnyFlagSource, FlagChange, FlagSource},
    AluResult,
};

/// The full product has twice the logical width of either input.
pub(crate) trait MultiplyType: MemoryInt {
    type Product: MemoryInt + AtLeast<Self> + AtLeast<I16>;
}

impl MultiplyType for I8 {
    type Product = I16;
}

impl MultiplyType for I16 {
    type Product = I32;
}

impl MultiplyType for I32 {
    type Product = I64;
}

#[derive(Clone, Copy)]
pub(crate) enum MultiplyOp {
    Signed,
    Unsigned,
}

impl MultiplyOp {
    pub(crate) fn apply<T: MultiplyType>(self, left: Val<T>, right: Val<T>) -> AluResult<T::Product>
    where
        FlagSource<T>: Into<AnyFlagSource>,
    {
        let result = match self {
            Self::Signed => left
                .signed()
                .extend::<T::Product>()
                .mul(right.signed().extend::<T::Product>()),
            Self::Unsigned => left
                .unsigned()
                .extend::<T::Product>()
                .mul(right.unsigned().extend::<T::Product>()),
        };
        let overflow = match self {
            Self::Signed => result.ne(result.truncate::<T>().signed().extend::<T::Product>()),
            Self::Unsigned => result.unsigned().shr(T::BYTES * 8).ne(0),
        };
        // PF/AF/ZF/SF are undefined. Choose 1/0/0/0 without reading old flags.
        let flags = FlagSource::<T>::Explicit {
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

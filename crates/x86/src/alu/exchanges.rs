//! Compare-exchange values and each instruction's status-flag contract.

use wasm86_compiler::{MemoryInt, Val, I64};

use super::{AnyStatusSource, ArithmeticOp, StatusSource};
use crate::flags::{Flag, FlagChange};

pub(crate) struct CompareExchangeResult<T: MemoryInt> {
    pub(crate) destination: Val<T>,
    pub(crate) accumulator: Val<T>,
    pub(crate) flags: FlagChange,
}

pub(crate) fn compare_exchange<T: MemoryInt>(
    destination: Val<T>,
    accumulator: Val<T>,
    replacement: Val<T>,
) -> CompareExchangeResult<T>
where
    StatusSource<T>: Into<AnyStatusSource>,
{
    let flags = ArithmeticOp::Subtract
        .apply(accumulator.clone(), destination.clone())
        .flags;
    exchange_values(destination, accumulator, replacement, flags)
}

pub(crate) fn compare_exchange8b(
    destination: Val<I64>,
    accumulator: Val<I64>,
    replacement: Val<I64>,
) -> CompareExchangeResult<I64> {
    let flags = FlagChange::partial([(Flag::ZF, accumulator.eq(&destination))]);
    exchange_values(destination, accumulator, replacement, flags)
}

fn exchange_values<T: MemoryInt>(
    destination: Val<T>,
    accumulator: Val<T>,
    replacement: Val<T>,
    flags: FlagChange,
) -> CompareExchangeResult<T> {
    let equal = accumulator.eq(&destination);
    CompareExchangeResult {
        destination: equal.select(replacement, &destination),
        accumulator: equal.select(accumulator, destination),
        flags,
    }
}

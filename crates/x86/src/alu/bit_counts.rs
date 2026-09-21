//! Population counts and their complete status-flag changes.

use wasm86_compiler::{MemoryInt, Val};

use super::{AluResult, AnyStatusSource, StatusSource};
use crate::flags::FlagChange;

pub(crate) fn population_count<T: MemoryInt>(source: Val<T>) -> AluResult<T>
where
    StatusSource<T>: Into<AnyStatusSource>,
{
    AluResult {
        result: source.popcnt(),
        flags: FlagChange::from(StatusSource::<T>::Explicit {
            flags: [
                false.into(),
                false.into(),
                false.into(),
                source.eq(0),
                false.into(),
                false.into(),
            ],
        }),
    }
}

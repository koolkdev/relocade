//! MMX instructions use raw physical x87 payloads and shared exception admission.

use super::*;
use crate::instruction::MmxLocation;
use crate::memory::TransferType;
use crate::register::RegisterCode;
use wasm86_compiler::{MemoryInt, I16, I64, V64};

mod binary;
mod minmax;
mod multiply;
mod pack;
mod shifts;
mod shuffle;
mod transfers;
mod unpack;
mod words;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    binary::forms()
        .chain(minmax::forms())
        .chain(multiply::forms())
        .chain(shifts::forms())
        .chain(shuffle::forms())
        .chain(pack::forms())
        .chain(transfers::forms())
        .chain(unpack::forms())
        .chain(words::forms())
}

fn binary<T: MemoryInt + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: MmxLocation,
    operation: impl FnOnce(Val<V64>, Val<V64>) -> Val<V64>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    execution.execute_mmx(|execution| {
        let right = source.read::<T>(execution)?.unsigned().extend::<I64>();
        let left = execution.read_mmx(destination.clone())?;
        let result = operation(Val::<V64>::from_bits(left), Val::<V64>::from_bits(right));
        execution.write_mmx(destination, result.to_bits())
    })
}

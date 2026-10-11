//! MMX instructions use raw physical x87 payloads and shared exception admission.

use super::*;
use crate::instruction::MmxLocation;
use crate::memory::TransferType;
use crate::register::RegisterCode;
use wasm86_compiler::{MemoryInt, I16, I64, V128};

mod binary;
mod multiply;
mod pack;
mod shifts;
mod transfers;
mod unpack;

pub(super) fn forms() -> impl Iterator<Item = &'static Form> + Clone {
    binary::forms()
        .chain(multiply::forms())
        .chain(shifts::forms())
        .chain(pack::forms())
        .chain(transfers::forms())
        .chain(unpack::forms())
}

fn binary<T: MemoryInt + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: MmxLocation,
    operation: impl FnOnce(Val<I64>, Val<I64>) -> Val<I64>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    execution.execute_mmx(|execution| {
        let right = source.read::<T>(execution)?.unsigned().extend::<I64>();
        let left = execution.read_mmx(destination.clone())?;
        execution.write_mmx(destination, operation(left, right))
    })
}

fn packed_binary<T: MemoryInt + TransferType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: RegisterCode,
    source: MmxLocation,
    operation: impl FnOnce(Val<V128>, Val<V128>) -> Val<V128>,
) -> Result<(), BuildError>
where
    I64: AtLeast<T>,
{
    binary::<T>(execution, destination, source, |left, right| {
        let left = Val::<V128>::from(0_u128).replace_lane(0, left);
        let right = Val::<V128>::from(0_u128).replace_lane(0, right);
        operation(left, right).extract_lane::<I64>(0)
    })
}

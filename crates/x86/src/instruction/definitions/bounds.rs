//! BOUND compares a signed register against an inclusive pair of memory bounds.

use super::*;
use crate::{
    address::MemoryAddress, exception::Exception, instruction::Location, register::RegisterType,
};

instruction_families! {
    BOUND {
        execute: check_bounds;
        forms { 0x62 => word_or_dword(modrm_reg, mem); }
    }
}

fn check_bounds<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    index: Input<T>,
    source: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let index = index.read(execution)?;
    let bounds = Location::Memory(source.into());
    // A failed lower comparison faults before accessing the upper bound. Each
    // bound starts at an address-sized offset; bytes within that bound stay consecutive.
    let lower = TypedLocation::<T>::new(bounds.clone()).read(execution)?;
    execution.fault_if(index.signed().lt(lower), Exception::BoundRangeExceeded)?;
    let upper = TypedLocation::<T>::new(bounds)
        .offset_memory(T::BYTES)
        .read(execution)?;
    execution.fault_if(upper.signed().lt(index), Exception::BoundRangeExceeded)
}

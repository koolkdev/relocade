//! Typed handler operands defer architectural accesses to the execution builder.

use std::marker::PhantomData;
use wasm86_compiler::{AtLeast, BuildError, Val, I32};

use super::{Location, Operand};
use crate::{
    address::{Address32, IndexTerm},
    execution::{ExecutionBuilder, PairValues},
    register::{Gpr32, RegisterType},
};

pub(crate) struct Input<T: RegisterType> {
    operand: Operand<Val<I32>>,
    width: PhantomData<T>,
}

impl<T: RegisterType> Input<T> {
    pub(crate) fn new(operand: Operand<Val<I32>>) -> Self {
        Self {
            operand,
            width: PhantomData,
        }
    }

    pub(crate) fn read(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<T>, BuildError>
    where
        I32: AtLeast<T>,
    {
        execution.read::<T>(self.operand)
    }

    pub(crate) fn into_operand(self) -> Operand<Val<I32>> {
        self.operand
    }
}

/// A `Location` with logical data width `T`; accesses remain deferred.
pub(crate) struct TypedLocation<T: RegisterType> {
    location: Location<Val<I32>>,
    width: PhantomData<T>,
}

impl<T: RegisterType> TypedLocation<T> {
    /// Selects the low `T`-width part of a named parent register.
    pub(crate) fn register(parent: Gpr32) -> Self {
        Self::new(Location::Register(parent.into()))
    }

    pub(crate) fn new(location: Location<Val<I32>>) -> Self {
        Self {
            location,
            width: PhantomData,
        }
    }

    pub(crate) fn from_operand(operand: Operand<Val<I32>>) -> Self {
        let Operand::Location(location) = operand else {
            unreachable!("the form binds a location for this handler")
        };
        Self::new(location)
    }

    /// Adds a wrapping byte offset to a memory location; registers are unchanged.
    /// Address registers and access permissions are still resolved at the access.
    pub(crate) fn offset_memory(mut self, offset: impl Into<Val<I32>>) -> Self {
        if let Location::Memory(address) = &mut self.location {
            address.displacement = address.displacement.add(offset);
        }
        self
    }

    pub(crate) fn read(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<T>, BuildError>
    where
        I32: AtLeast<T>,
    {
        execution.read::<T>(self.location.into())
    }

    pub(crate) fn write(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        execution.write::<T>(self.location, value)
    }

    pub(crate) fn update<'body, 'module>(
        self,
        execution: &mut ExecutionBuilder<'body, 'module>,
        update: impl FnOnce(&mut ExecutionBuilder<'body, 'module>, Val<T>) -> Result<Val<T>, BuildError>,
    ) -> Result<(), BuildError> {
        execution.update::<T>(self.location, update)
    }

    pub(crate) fn update_pair<'body, 'module>(
        self,
        execution: &mut ExecutionBuilder<'body, 'module>,
        other: Self,
        update: impl FnOnce(
            &mut ExecutionBuilder<'body, 'module>,
            PairValues<T>,
        ) -> Result<PairValues<T>, BuildError>,
    ) -> Result<(), BuildError> {
        execution.update_pair::<T>(self.location, other.location, update)
    }

    pub(crate) fn into_location(self) -> Location<Val<I32>> {
        self.location
    }
}

pub(super) fn map_operand<V: Into<Val<I32>>>(operand: Operand<V>) -> Operand<Val<I32>> {
    match operand {
        Operand::Immediate(bits) => Operand::Immediate(bits.into()),
        Operand::Address(address) => Operand::Address(map_address(address)),
        Operand::Location(location) => map_location(location).into(),
    }
}

pub(super) fn map_location<V: Into<Val<I32>>>(location: Location<V>) -> Location<Val<I32>> {
    match location {
        Location::Register(register) => Location::Register(register),
        Location::Memory(address) => Location::Memory(map_address(address)),
    }
}

fn map_address<V: Into<Val<I32>>>(address: Address32<V>) -> Address32<Val<I32>> {
    Address32 {
        base: address.base,
        index: address.index.map(|index| IndexTerm {
            register: index.register,
            shift: index.shift.into(),
        }),
        displacement: address.displacement.into(),
    }
}

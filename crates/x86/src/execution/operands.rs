//! Operand accesses share address resolution and permission checks.

use wasm86_compiler::{AtLeast, BuildError, Val, I32};

use crate::{
    address::{self, RegisterValue},
    alu::OperandUpdate,
    instruction::{Location, Operand},
    register::{Register, RegisterType},
};

use super::{memory::MemoryOperand, ExecutionBuilder};

/// A location whose complete write span has passed its architectural guards.
pub(crate) struct WriteTarget<'memory, T: RegisterType> {
    location: WriteLocation<'memory, T>,
}

enum WriteLocation<'memory, T: RegisterType> {
    Register(Register<T>),
    Memory(MemoryOperand<'memory>),
}

impl<T: RegisterType> WriteTarget<'_, T> {
    pub(crate) fn read(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<T>, BuildError> {
        match &self.location {
            WriteLocation::Register(register) => execution
                .state
                .read_register(&mut execution.body, register.clone()),
            WriteLocation::Memory(target) => target.read(execution, 0),
        }
    }

    /// Applies a prepared modification and publishes effects from its prior value.
    /// The callback must not perform faulting guest accesses. For register targets,
    /// it runs before the final write so the destination wins register aliases.
    pub(crate) fn modify(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        update: OperandUpdate<T>,
        locked: bool,
        complete: impl FnOnce(&mut ExecutionBuilder<'_, '_>, Val<T>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        match self.location {
            WriteLocation::Memory(target) if locked => {
                let previous = target.atomic_update(execution, &update)?;
                complete(execution, previous)
            }
            location => {
                let target = Self { location };
                let previous = target.read(execution)?;
                let replacement = update.apply(&previous);
                complete(execution, previous)?;
                target.write(execution, replacement)
            }
        }
    }

    /// Writes without repeating address evaluation or architectural guards.
    /// Register targets retain their alias and apply it to state at this point.
    pub(crate) fn write(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        match self.location {
            WriteLocation::Register(register) => {
                execution
                    .state
                    .write_register(&mut execution.body, register, value)
            }
            WriteLocation::Memory(target) => target.write(execution, 0, value),
        }
    }
}

impl<'memory> ExecutionBuilder<'_, 'memory> {
    pub(crate) fn read<T: RegisterType>(
        &mut self,
        operand: Operand<impl Into<Val<I32>>>,
    ) -> Result<Val<T>, BuildError>
    where
        I32: AtLeast<T>,
    {
        match operand {
            Operand::Segment(_) => unreachable!("segment operands use selector operations"),
            Operand::X87StackIndex(_) => unreachable!("x87 stack operands use stack operations"),
            Operand::Immediate(bits) => Ok(self.body.value::<I32>(bits)?.truncate::<T>()),
            Operand::Address(address) => {
                let address = address::resolve(&mut self.body, &mut self.state, address, &[])?;
                Ok(address.truncate::<T>())
            }
            Operand::Location(Location::Register(register)) => self
                .state
                .read_register(&mut self.body, register.view::<T>()),
            Operand::Location(Location::Memory(address)) => self.read_memory::<T>(*address),
        }
    }

    pub(crate) fn write<T: RegisterType>(
        &mut self,
        location: Location<impl Into<Val<I32>>>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let target = self.prepare_write::<T>(location, &[])?;
        target.write(self, value)
    }

    /// Checks the complete write span before reading the old value, then writes
    /// the callback's result through the same target. The callback must read any
    /// other faulting operands before changing architectural state.
    pub(crate) fn update<T: RegisterType>(
        &mut self,
        location: Location<impl Into<Val<I32>>>,
        update: impl FnOnce(&mut Self, Val<T>) -> Result<Val<T>, BuildError>,
    ) -> Result<(), BuildError> {
        let target = self.prepare_write::<T>(location, &[])?;
        let old_value = target.read(self)?;
        let value = update(self, old_value)?;
        target.write(self, value)
    }

    /// Resolves the destination using temporary address bindings and checks its
    /// complete write span without changing architectural register values.
    pub(crate) fn prepare_write<T: RegisterType>(
        &mut self,
        location: Location<impl Into<Val<I32>>>,
        bindings: &[RegisterValue],
    ) -> Result<WriteTarget<'memory, T>, BuildError> {
        let location = match location {
            Location::Register(register) => WriteLocation::Register(register.view::<T>()),
            Location::Memory(address) => {
                WriteLocation::Memory(self.prepare_memory_write::<T>(*address, bindings)?)
            }
        };
        Ok(WriteTarget { location })
    }
}

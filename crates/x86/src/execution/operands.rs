//! Operand accesses share address resolution and permission checks.

use wasm86_compiler::{AtLeast, BuildError, Val, I32};

use crate::{
    address::{self, RegisterValue},
    instruction::{Location, Operand},
    memory::{Access, Intent, Memory},
    register::{Register, RegisterType},
};

use super::ExecutionBuilder;

/// Values read from or written to a pair of locations.
pub(crate) struct PairValues<T: RegisterType> {
    pub(crate) left: Val<T>,
    pub(crate) right: Val<T>,
}

/// A location whose complete write span has passed its architectural guards.
pub(crate) struct WriteTarget<'memory, T: RegisterType> {
    location: WriteLocation<'memory, T>,
}

enum WriteLocation<'memory, T: RegisterType> {
    Register(Register<T>),
    Memory {
        memory: &'memory Memory,
        access: Access,
    },
}

impl<T: RegisterType> WriteTarget<'_, T> {
    fn read(&self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<T>, BuildError> {
        match &self.location {
            WriteLocation::Register(register) => execution
                .state
                .read_register(&mut execution.body, register.clone()),
            WriteLocation::Memory { memory, access } => {
                memory.read::<T>(&mut execution.body, access, 0)
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
            WriteLocation::Memory { memory, access } => {
                let value = execution.body.value(value)?;
                memory.write(&mut execution.body, &access, 0, &value)
            }
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
            Operand::Immediate(bits) => Ok(self.body.value::<I32>(bits)?.truncate::<T>()),
            Operand::Address(address) => {
                let address = address::resolve(&mut self.body, &mut self.state, address, &[])?;
                Ok(address.truncate::<T>())
            }
            Operand::Location(Location::Register(register)) => self
                .state
                .read_register(&mut self.body, register.view::<T>()),
            Operand::Location(Location::Memory(address)) => {
                let offset =
                    address::resolve(&mut self.body, &mut self.state, address.offset, &[])?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access =
                    self.checked(memory, &address.segment, &offset, T::BYTES, Intent::Read)?;
                memory.read::<T>(&mut self.body, &access, 0)
            }
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

    /// Resolves and checks both write targets, then reads both old values before
    /// calling the update. Writes right before left, so left wins when they alias.
    /// The callback must read any other faulting operands before changing state.
    pub(crate) fn update_pair<T: RegisterType>(
        &mut self,
        left: Location<impl Into<Val<I32>>>,
        right: Location<impl Into<Val<I32>>>,
        update: impl FnOnce(&mut Self, PairValues<T>) -> Result<PairValues<T>, BuildError>,
    ) -> Result<(), BuildError> {
        let left = self.prepare_write::<T>(left, &[])?;
        let right = self.prepare_write::<T>(right, &[])?;
        let old_values = PairValues {
            left: left.read(self)?,
            right: right.read(self)?,
        };
        let values = update(self, old_values)?;
        right.write(self, values.right)?;
        left.write(self, values.left)
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
                let offset =
                    address::resolve(&mut self.body, &mut self.state, address.offset, bindings)?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access =
                    self.checked(memory, &address.segment, &offset, T::BYTES, Intent::Write)?;
                WriteLocation::Memory { memory, access }
            }
        };
        Ok(WriteTarget { location })
    }
}

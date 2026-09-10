//! Operand reads, writes and updates share address and permission checks.

use wasm86_compiler::{AtLeast, BuildError, Val, I32};

use crate::{
    address::{self, RegisterValue},
    instruction::{Location, Operand},
    memory::{Access, Intent, Memory},
    register::{Register, RegisterType},
};

use super::ExecutionBuilder;

/// A location whose complete write span has passed its architectural guards.
pub(super) enum WriteTarget<'memory, T: RegisterType> {
    Register(Register<T>),
    Memory {
        memory: &'memory Memory,
        access: Access<T>,
    },
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
            Operand::Immediate(bits) => Ok(self.body.value::<I32>(bits)?.truncate::<T>()),
            Operand::Address(address) => {
                let address = address::resolve(&mut self.body, &mut self.state, address, &[])?;
                Ok(address.truncate::<T>())
            }
            Operand::Location(Location::Register(code)) => {
                self.state.read_register(&mut self.body, code.view::<T>())
            }
            Operand::Location(Location::Memory(address)) => {
                let address = address::resolve(&mut self.body, &mut self.state, address, &[])?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access = self.checked::<T>(memory, &address, Intent::Read)?;
                memory.read(&mut self.body, &access)
            }
        }
    }

    pub(crate) fn write<T: RegisterType>(
        &mut self,
        location: Location<impl Into<Val<I32>>>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let target = self.prepare_write::<T>(location, &[])?;
        self.write_target(target, value)
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
        let old_value = self.read_target(&target)?;
        let value = update(self, old_value)?;
        self.write_target(target, value)
    }

    pub(super) fn prepare_write<T: RegisterType>(
        &mut self,
        location: Location<impl Into<Val<I32>>>,
        bindings: &[RegisterValue],
    ) -> Result<WriteTarget<'memory, T>, BuildError> {
        Ok(match location {
            Location::Register(code) => WriteTarget::Register(code.view::<T>()),
            Location::Memory(address) => {
                let address = address::resolve(&mut self.body, &mut self.state, address, bindings)?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access = self.checked::<T>(memory, &address, Intent::Write)?;
                WriteTarget::Memory { memory, access }
            }
        })
    }

    fn read_target<T: RegisterType>(
        &mut self,
        target: &WriteTarget<'memory, T>,
    ) -> Result<Val<T>, BuildError> {
        match target {
            WriteTarget::Register(register) => {
                self.state.read_register(&mut self.body, register.clone())
            }
            WriteTarget::Memory { memory, access } => memory.read(&mut self.body, access),
        }
    }

    /// Applies a prepared target without further architectural guards.
    pub(super) fn write_target<T: RegisterType>(
        &mut self,
        target: WriteTarget<'memory, T>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        match target {
            WriteTarget::Register(register) => {
                self.state.write_register(&mut self.body, register, value)
            }
            WriteTarget::Memory { memory, access } => {
                let value = self.body.value(value)?;
                memory.write(&mut self.body, &access, &value)
            }
        }
    }
}

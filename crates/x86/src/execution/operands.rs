//! Operand reads, writes and updates share address and permission checks.

use wasm86_compiler::{AtLeast, BuildError, IntoOp, Val, I32};

use crate::{
    address,
    instruction::{Location, Operand},
    memory::{Access, Intent, Memory},
    register::{Register, RegisterType},
    state::exit,
};

use super::ExecutionBuilder;

/// A location whose complete write span has passed its architectural guards.
enum WriteTarget<T: RegisterType> {
    Register(Register<T>),
    Memory { memory: Memory, access: Access<T> },
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn read<T: RegisterType>(
        &mut self,
        operand: Operand<impl IntoOp<I32>>,
    ) -> Result<Val<T>, BuildError>
    where
        I32: AtLeast<T>,
    {
        match operand {
            Operand::Immediate(bits) => Ok(self.body.value::<I32>(bits)?.truncate::<T>()),
            Operand::Location(Location::Register(code)) => {
                self.state.read_register(&mut self.body, code.view::<T>())
            }
            Operand::Location(Location::Memory(address)) => {
                let address = address::resolve(&mut self.body, &mut self.state, address)?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access = self.checked::<T>(memory, &address, Intent::Read)?;
                memory.read(&mut self.body, &access)
            }
        }
    }

    pub(crate) fn write<T: RegisterType>(
        &mut self,
        location: Location<impl IntoOp<I32>>,
        value: impl IntoOp<T>,
    ) -> Result<(), BuildError> {
        let target = self.prepare_write::<T>(location)?;
        self.write_target(target, value)
    }

    /// Checks the complete write span before reading the old value, then writes
    /// the callback's result through the same target. The callback must read any
    /// other faulting operands before changing architectural state.
    pub(crate) fn update<T: RegisterType>(
        &mut self,
        location: Location<impl IntoOp<I32>>,
        update: impl FnOnce(&mut Self, Val<T>) -> Result<Val<T>, BuildError>,
    ) -> Result<(), BuildError> {
        let target = self.prepare_write::<T>(location)?;
        let old_value = self.read_target(&target)?;
        let value = update(self, old_value)?;
        self.write_target(target, value)
    }

    fn prepare_write<T: RegisterType>(
        &mut self,
        location: Location<impl IntoOp<I32>>,
    ) -> Result<WriteTarget<T>, BuildError> {
        Ok(match location {
            Location::Register(code) => WriteTarget::Register(code.view::<T>()),
            Location::Memory(address) => {
                let address = address::resolve(&mut self.body, &mut self.state, address)?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access = self.checked::<T>(memory, &address, Intent::Write)?;
                WriteTarget::Memory { memory, access }
            }
        })
    }

    fn read_target<T: RegisterType>(
        &mut self,
        target: &WriteTarget<T>,
    ) -> Result<Val<T>, BuildError> {
        match target {
            WriteTarget::Register(register) => {
                self.state.read_register(&mut self.body, register.clone())
            }
            WriteTarget::Memory { memory, access } => memory.read(&mut self.body, access),
        }
    }

    fn write_target<T: RegisterType>(
        &mut self,
        target: WriteTarget<T>,
        value: impl IntoOp<T>,
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

    fn checked<T: RegisterType>(
        &mut self,
        memory: Memory,
        address: &Val<I32>,
        intent: Intent,
    ) -> Result<Access<T>, BuildError> {
        let access = memory.resolve_access::<T>(&mut self.body, address, intent)?;
        self.body.if_(&access.fault.condition, |mut arm| {
            self.state.publish(&mut arm, &self.eip, self.completed)?;
            arm.return_(exit::page_fault(&access.fault.address, &access.fault.error))
        })?;
        Ok(access)
    }
}

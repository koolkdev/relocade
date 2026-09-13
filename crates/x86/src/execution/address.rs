//! Address-sized implicit registers use the ordinary memory-backed aliases.

use super::ExecutionBuilder;
use crate::{
    address::AddressSize,
    register::{Gpr32, Register},
};
use wasm86_compiler::{BuildError, Val, I16, I32};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn address_size(&self) -> AddressSize {
        self.address_size
    }

    pub(crate) fn read_address_register(
        &mut self,
        register: Gpr32,
    ) -> Result<Val<I32>, BuildError> {
        match self.address_size {
            AddressSize::Bits16 => Ok(self
                .state
                .read_register::<I16>(&mut self.body, Register::named(register))?
                .unsigned()
                .extend::<I32>()),
            AddressSize::Bits32 => self.state.read_register(&mut self.body, register),
        }
    }

    pub(crate) fn write_address_register(
        &mut self,
        register: Gpr32,
        value: Val<I32>,
    ) -> Result<(), BuildError> {
        match self.address_size {
            AddressSize::Bits16 => self.state.write_register(
                &mut self.body,
                Register::named(register),
                value.truncate::<I16>(),
            ),
            AddressSize::Bits32 => self.state.write_register(&mut self.body, register, value),
        }
    }
}

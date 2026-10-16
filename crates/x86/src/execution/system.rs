//! System-register queries and their complete guest-memory stores.

use super::ExecutionBuilder;
use crate::{
    address::MemoryAddress, memory::Intent, register::RegisterType, DescriptorTable,
    DescriptorTableRegister, ExecutionProfile, SystemSelector,
};
use wasm86_compiler::{BuildError, Val, I16, I32};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn query_system_selector(
        &mut self,
        register: SystemSelector,
    ) -> Result<Val<I16>, BuildError> {
        self.runtime.query_system_selector(&mut self.body, register)
    }

    pub(crate) fn store_descriptor_table<T: RegisterType>(
        &mut self,
        destination: MemoryAddress<Val<I32>>,
        table: DescriptorTable,
    ) -> Result<(), BuildError> {
        let operand = self.memory_operand(destination, 6, Intent::Write, &[])?;
        let register = match (self.profile(), table) {
            (ExecutionProfile::Real16, DescriptorTable::Interrupt) => DescriptorTableRegister {
                // Real16 interrupt delivery uses the conventional 256-vector IVT.
                base: 0.into(),
                limit: 0x3ff.into(),
            },
            _ => self.runtime.query_descriptor_table(&mut self.body, table)?,
        };
        let base = if T::BYTES == 2 {
            // P4's 16-bit form stores a 24-bit base and clears the sixth byte.
            register.base.and(0x00ff_ffff)
        } else {
            register.base
        };
        operand.write::<I16>(self, 0, register.limit)?;
        operand.write::<I32>(self, 2, base)
    }
}

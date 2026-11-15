//! Reads of architectural system registers from the retained CPU image.

use super::{
    access::{cpu_load, cpu_location},
    cr0, cr4, State, StateAssumption,
};
use crate::PrivilegeLevel;
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I8};
use std::sync::Arc;

#[cfg(test)]
mod tests;

pub(crate) struct DescriptorTableValues {
    pub limit: Val<I16>,
    pub base: Val<I32>,
}

impl State<'_> {
    /// System controls cannot change within an admitted entry. Segment loads
    /// dispatch before any further reads of the old CS selector.
    pub(crate) fn with_control_assumptions(mut self, assumptions: Arc<[StateAssumption]>) -> Self {
        self.control_assumptions = Some(assumptions);
        self
    }

    pub(crate) fn protection_enabled(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        if let Some(assumptions) = self.control_assumptions.take() {
            for assumption in assumptions.iter() {
                assumption.apply(body, &mut self.system)?;
            }
        }
        let cr0 = self.system.read(body, cpu_location!(cr0))?;
        Ok(cr0.and(cr0::PROTECTION_ENABLE).ne(0))
    }

    pub(crate) fn protected_mode(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        let protected = self.protection_enabled(body)?;
        let vm = self.virtual_8086_flag(body)?;
        Ok(protected.and(vm.eq(false)))
    }

    pub(crate) fn privilege_level(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I8>, BuildError> {
        let protected = self.protection_enabled(body)?;
        let vm = self.virtual_8086_flag(body)?;
        let selector = self
            .system
            .read(body, cpu_location!(segments.cs.selector))?;
        let selector_privilege = selector
            .truncate::<I8>()
            .and(u32::from(PrivilegeLevel::MASK));
        let protected_privilege = vm.select(PrivilegeLevel::Ring3 as u32, selector_privilege);
        Ok(protected.select(protected_privilege, PrivilegeLevel::Ring0 as u32))
    }

    pub(crate) fn timestamp_access_denied(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        let cpl = self.privilege_level(body)?;
        let timestamp_disabled = self
            .system
            .read(body, cpu_location!(cr4))?
            .and(cr4::TIMESTAMP_DISABLE)
            .ne(0);
        Ok(cpl.ne(PrivilegeLevel::Ring0 as u32).and(timestamp_disabled))
    }

    fn virtual_8086_flag(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I1>, BuildError> {
        Ok(self
            .system
            .read(body, cpu_location!(flags.bytes.vm))?
            .truncate())
    }

    pub(crate) fn machine_status_word(
        &self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        Ok(cpu_load!(body, self.cpu.memory(), cr0)?.truncate())
    }

    pub(crate) fn global_descriptor_table(
        &self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<DescriptorTableValues, BuildError> {
        Ok(DescriptorTableValues {
            limit: cpu_load!(body, self.cpu.memory(), gdtr.limit)?,
            base: cpu_load!(body, self.cpu.memory(), gdtr.base)?,
        })
    }
}

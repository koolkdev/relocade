//! Reads of architectural system registers from the retained CPU image.

use super::{
    access::{cpu_load, cpu_location},
    cr0, cr4, State,
};
use crate::ssa::StateFields;
use crate::PrivilegeLevel;
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I8};

#[cfg(test)]
mod tests;

pub(crate) struct DescriptorTableValues {
    pub limit: Val<I16>,
    pub base: Val<I32>,
}

fn protection_enabled(
    body: &mut BlockBuilder<'_>,
    fields: &mut StateFields,
) -> Result<Val<I1>, BuildError> {
    let cr0 = fields.read(body, cpu_location!(cr0))?;
    Ok(cr0.and(cr0::PROTECTION_ENABLE).ne(0))
}

fn virtual_8086_flag(
    body: &mut BlockBuilder<'_>,
    fields: &mut StateFields,
) -> Result<Val<I1>, BuildError> {
    Ok(fields.read(body, cpu_location!(flags.bytes.vm))?.truncate())
}

pub(crate) fn protected_mode(
    body: &mut BlockBuilder<'_>,
    fields: &mut StateFields,
) -> Result<Val<I1>, BuildError> {
    let protected = protection_enabled(body, fields)?;
    let vm = virtual_8086_flag(body, fields)?;
    Ok(protected.and(vm.eq(false)))
}

/// Entry assumptions can make the loaded CS default constant.
pub(super) fn code_default_is_32_bit(
    body: &mut BlockBuilder<'_>,
    fields: &mut StateFields,
) -> Result<Val<I1>, BuildError> {
    Ok(fields.read(body, cpu_location!(segments.cs.big))?.ne(0))
}

impl State<'_> {
    pub(crate) fn code_default_is_32_bit(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        code_default_is_32_bit(body, &mut self.fields)
    }

    pub(crate) fn protection_enabled(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        protection_enabled(body, &mut self.fields)
    }

    pub(crate) fn protected_mode(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        protected_mode(body, &mut self.fields)
    }

    pub(crate) fn privilege_level(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I8>, BuildError> {
        let protected = self.protection_enabled(body)?;
        let vm = virtual_8086_flag(body, &mut self.fields)?;
        let selector = self
            .fields
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
            .fields
            .read(body, cpu_location!(cr4))?
            .and(cr4::TIMESTAMP_DISABLE)
            .ne(0);
        Ok(cpl.ne(PrivilegeLevel::Ring0 as u32).and(timestamp_disabled))
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

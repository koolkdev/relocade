//! Reads of architectural system registers from the retained CPU image.

use super::{access::cpu_load, State};
use relocade_compiler::{BlockBuilder, BuildError, Val, I16, I32};

pub(crate) struct DescriptorTableValues {
    pub limit: Val<I16>,
    pub base: Val<I32>,
}

impl State<'_> {
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

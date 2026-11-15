//! CPU capabilities and processor execution control.

use super::ExecutionBuilder;
use crate::{
    processor::{self, CpuidValues},
    state::DescriptorTableValues,
    CpuFeature, ExecutionExitValues, PrivilegeLevel,
};
use relocade_compiler::{BuildError, Val, I1, I16, I32, I8};

#[cfg(test)]
mod tests;

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn protected_mode(&mut self) -> Result<Val<I1>, BuildError> {
        self.state.protected_mode(&mut self.body)
    }

    pub(crate) fn privilege_level(&mut self) -> Result<Val<I8>, BuildError> {
        self.state.privilege_level(&mut self.body)
    }

    pub(crate) fn io_privilege_denied(&mut self) -> Result<Val<I1>, BuildError> {
        let protected = self.state.protection_enabled(&mut self.body)?;
        let cpl = self.state.privilege_level(&mut self.body)?;
        // The current protected execution policy fixes effective IOPL at zero;
        // saved IOPL bytes remain available to real-mode flag operations.
        let iopl = self.state.read_iopl(&mut self.body)?;
        let effective_iopl = protected.select(PrivilegeLevel::Ring0 as u32, iopl);
        Ok(effective_iopl.unsigned().lt(cpl))
    }

    pub(crate) fn timestamp_access_denied(&mut self) -> Result<Val<I1>, BuildError> {
        self.state.timestamp_access_denied(&mut self.body)
    }

    pub(crate) fn machine_status_word(&mut self) -> Result<Val<I16>, BuildError> {
        self.state.machine_status_word(&mut self.body)
    }

    pub(crate) fn global_descriptor_table(&mut self) -> Result<DescriptorTableValues, BuildError> {
        self.state.global_descriptor_table(&mut self.body)
    }

    pub(crate) fn halt(&mut self, next_ip: &Val<I32>) -> Result<(), BuildError> {
        self.state
            .publish(&mut self.body, next_ip, self.completed + 1)?;
        // The driver must retain the halt before observing host notifications.
        self.body
            .block::<()>(|body, _| body.return_(ExecutionExitValues::halted()))
    }

    /// Makes generated interpreter code read feature bits from the guest context.
    /// Snapshot builders leave the model fixed to the compilation target.
    pub(crate) fn with_cpu_features_from_context(mut self) -> Result<Self, BuildError> {
        self.cpu_features = self.state.read_cpu_features(&mut self.body)?;
        Ok(self)
    }

    pub(crate) fn supports_cpu_features(&self, features: &[CpuFeature]) -> Val<I1> {
        processor::supports_features(&self.cpu_features, features)
    }

    pub(crate) fn cpuid(&self, leaf: &Val<I32>) -> CpuidValues {
        processor::cpuid(&self.cpu_features, leaf)
    }
}

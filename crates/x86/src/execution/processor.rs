//! CPU capabilities and processor execution control.

use super::ExecutionBuilder;
use crate::{
    processor::{self, CpuidValues},
    CpuFeature, Exception, ExecutionExitValues, ExecutionProfile,
};
use relocade_compiler::{BuildError, Val, I1, I32, I64, I8};

impl ExecutionBuilder<'_, '_> {
    /// Admission establishes these conditions; supported instructions preserve
    /// them until dispatch.
    pub(super) fn assume_entry_state(
        &mut self,
        profile: ExecutionProfile,
    ) -> Result<(), BuildError> {
        let protected = matches!(profile, ExecutionProfile::Protected(_));
        let cr0 = self.state.read_cr0(&mut self.body)?;
        let cr4 = self.state.read_cr4(&mut self.body)?;
        let virtual_8086 = self.state.virtual_8086(&mut self.body)?;
        let mut condition = cr0
            .eq(0x30 | u32::from(protected))
            .and(cr4.eq(0x600))
            .and(virtual_8086.eq(false));
        if protected {
            condition = condition.and(self.privilege_level()?.eq(3));
        }
        self.body.assume(condition)
    }

    pub(crate) fn protected_mode(&mut self) -> Result<Val<I1>, BuildError> {
        self.state.protected_mode(&mut self.body)
    }

    pub(crate) fn privilege_level(&mut self) -> Result<Val<I8>, BuildError> {
        self.state.privilege_level(&mut self.body)
    }

    /// Protected execution exposes IOPL0 as host policy; the backing flags may differ.
    pub(crate) fn effective_iopl(&mut self) -> Result<Val<I8>, BuildError> {
        Ok(self.protected_mode()?.select(0, self.read_iopl()?))
    }

    pub(crate) fn timestamp_counter(&mut self) -> Result<Val<I64>, BuildError> {
        let restricted = self.state.read_cr4(&mut self.body)?.and(4).ne(0);
        let unprivileged = self.privilege_level()?.ne(0);
        self.fault_if(
            restricted.and(unprivileged),
            Exception::GeneralProtection { error_code: 0 },
        )?;
        self.instruction_count()
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

//! CPU capabilities and processor execution control.

use super::ExecutionBuilder;
use crate::{
    processor::{self, CpuidValues},
    state::DescriptorTableValues,
    CpuFeature, ExecutionExitValues,
};
use relocade_compiler::{BuildError, Val, I1, I32};

impl ExecutionBuilder<'_, '_> {
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

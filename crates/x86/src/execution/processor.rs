//! CPU capabilities are fixed in snapshot blocks and read from backing in interpreters.

use super::ExecutionBuilder;
use crate::{
    processor::{self, CpuidValues},
    CpuFeature,
};
use relocade_compiler::{BuildError, Val, I1, I32};

impl ExecutionBuilder<'_, '_> {
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

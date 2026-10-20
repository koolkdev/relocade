//! CPU capabilities are fixed in snapshot blocks and read from backing in interpreters.

use super::ExecutionBuilder;
use crate::{
    processor::{self, CpuidValues},
    CpuFeature,
};
use relocade_compiler::{BuildError, Val, I1, I32};

/// Availability of required guest CPU features, as known during code generation.
/// Instruction lowering uses this result to emit faults before operand access.
pub(crate) enum CpuFeatureCheck {
    /// The fixed model enables the features, or no features are required.
    Supported,
    /// The fixed model disables at least one required feature.
    Unsupported,
    /// A generated condition that is true when a required guest feature is disabled.
    Runtime { missing: Val<I1> },
}

impl ExecutionBuilder<'_, '_> {
    /// Makes generated interpreter code read feature bits from the guest context.
    /// Snapshot builders leave the model fixed to the compilation target.
    pub(crate) fn with_cpu_features_from_context(mut self) -> Result<Self, BuildError> {
        self.runtime_cpu_features = Some(self.state.read_cpu_features(&mut self.body)?);
        Ok(self)
    }

    /// Resolves feature requirements without emitting faults or instruction effects.
    pub(crate) fn cpu_feature_check(&self, features: &[CpuFeature]) -> CpuFeatureCheck {
        if let Some(bits) = &self.runtime_cpu_features {
            let required = features
                .iter()
                .fold(0, |mask, feature| mask | feature.mask());
            if required == 0 {
                CpuFeatureCheck::Supported
            } else {
                CpuFeatureCheck::Runtime {
                    missing: bits.and(required).ne(required),
                }
            }
        } else if features
            .iter()
            .any(|&feature| !self.target.cpu_model.supports(feature))
        {
            CpuFeatureCheck::Unsupported
        } else {
            CpuFeatureCheck::Supported
        }
    }

    pub(crate) fn cpuid(&self, leaf: &Val<I32>) -> CpuidValues {
        let features = self
            .runtime_cpu_features
            .clone()
            .unwrap_or_else(|| self.target.cpu_model.bits().into());
        processor::cpuid(&features, leaf)
    }
}

//! CPU capabilities are fixed in snapshot blocks and read from backing in interpreters.

use super::ExecutionBuilder;
use crate::{
    processor::{self, CpuidValues},
    CpuFeature, Exception,
};
use relocade_compiler::{BuildError, Val, I32};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn with_runtime_cpu_model(mut self) -> Result<Self, BuildError> {
        self.runtime_cpu_features = Some(self.state.read_cpu_features(&mut self.body)?);
        Ok(self)
    }

    /// Return false only when a fixed model rejects the instruction outright.
    /// Runtime rejection exits before operand access or instruction effects.
    pub(crate) fn require_cpu_features(
        &mut self,
        features: &[CpuFeature],
    ) -> Result<bool, BuildError> {
        if let Some(bits) = &self.runtime_cpu_features {
            let required = features
                .iter()
                .fold(0, |mask, feature| mask | feature.mask());
            if required != 0 {
                self.fault_if(bits.and(required).ne(required), Exception::InvalidOpcode)?;
            }
            Ok(true)
        } else if features
            .iter()
            .any(|&feature| !self.target.cpu_model.supports(feature))
        {
            self.fault(Exception::InvalidOpcode)?;
            Ok(false)
        } else {
            Ok(true)
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

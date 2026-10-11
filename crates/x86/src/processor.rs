//! Capabilities selected for the virtual processor, independent of execution mode.

mod cpuid;
mod feature;

pub use feature::CpuFeature;

/// Immutable capability choices for one virtual processor.
///
/// The default enables all complete optional capabilities. Disabling one changes
/// CPUID reporting and instruction availability in blocks and interpreters.
/// Baseline instructions and existing partial x87/SSE implementations remain
/// available; feature selection does not emulate an older processor generation.
///
/// The capability catalog also names incomplete and unimplemented groups. Enabling
/// one returns [`UnsupportedCpuFeature`] instead of advertising missing behavior.
/// Keep this model fixed for a guest and include it in compiled-entry cache keys.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CpuModel {
    features: u16,
}

impl Default for CpuModel {
    fn default() -> Self {
        Self {
            features: feature::IMPLEMENTED_FEATURES,
        }
    }
}

impl CpuModel {
    /// Enable or disable one architectural capability.
    ///
    /// Returns [`UnsupportedCpuFeature`] when enabling an incomplete or unimplemented
    /// capability. Disabling an unavailable capability leaves the model unchanged.
    ///
    /// ```
    /// use wasm86_x86::{CpuFeature, CpuModel, UnsupportedCpuFeature};
    /// let model = CpuModel::default().with_feature(CpuFeature::Cmov, false)?;
    /// assert!(!model.supports(CpuFeature::Cmov));
    /// assert_eq!(
    ///     model.with_feature(CpuFeature::Sse2, true),
    ///     Err(UnsupportedCpuFeature { feature: CpuFeature::Sse2 }),
    /// );
    /// # Ok::<(), UnsupportedCpuFeature>(())
    /// ```
    pub const fn with_feature(
        mut self,
        feature: CpuFeature,
        enabled: bool,
    ) -> Result<Self, UnsupportedCpuFeature> {
        if enabled && !feature.is_implemented() {
            return Err(UnsupportedCpuFeature { feature });
        }
        if enabled {
            self.features |= feature.mask();
        } else {
            self.features &= !feature.mask();
        }
        Ok(self)
    }

    /// Whether this model enables the complete capability.
    pub const fn supports(self, feature: CpuFeature) -> bool {
        self.features & feature.mask() != 0
    }
}

/// A requested CPU capability is not completely implemented by this build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedCpuFeature {
    /// The capability that could not be enabled.
    pub feature: CpuFeature,
}

impl std::fmt::Display for UnsupportedCpuFeature {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the {:?} CPU capability is not fully implemented",
            self.feature
        )
    }
}

impl std::error::Error for UnsupportedCpuFeature {}

#[cfg(test)]
mod tests;

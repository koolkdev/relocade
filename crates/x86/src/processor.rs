//! Capabilities selected for the virtual processor, independent of execution mode.

mod cpuid;
mod feature;

pub use feature::CpuFeature;

/// Immutable capability choices for one virtual processor.
///
/// The default enables every [`CpuFeature`]. CPUID reports the enabled capabilities;
/// disabling one rejects its instructions in blocks and interpreters.
///
/// Choices are independent instruction extensions, not older processor generations
/// or register-bank switches. They do not remove stored x87, MMX or XMM state.
/// Keep this model fixed for a guest and include it in compiled-entry cache keys.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CpuModel {
    features: u16,
}

impl Default for CpuModel {
    fn default() -> Self {
        Self {
            features: feature::ALL_FEATURES,
        }
    }
}

impl CpuModel {
    /// Enable or disable one architectural capability.
    ///
    /// ```
    /// use wasm86_x86::{CpuFeature, CpuModel};
    /// let model = CpuModel::default().with_feature(CpuFeature::Cmov, false);
    /// assert!(!model.supports(CpuFeature::Cmov));
    /// ```
    #[must_use]
    pub const fn with_feature(mut self, feature: CpuFeature, enabled: bool) -> Self {
        if enabled {
            self.features |= feature.mask();
        } else {
            self.features &= !feature.mask();
        }
        self
    }

    /// Whether this model enables the capability.
    pub const fn supports(self, feature: CpuFeature) -> bool {
        self.features & feature.mask() != 0
    }
}

#[cfg(test)]
mod tests;

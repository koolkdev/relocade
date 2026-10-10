//! Capabilities selected for the virtual processor, independent of execution mode.

mod cpuid;

/// A complete optional capability implemented by the virtual processor.
///
/// Identifiers are independent of their architectural CPUID bit positions.
/// Partial x87 and SSE implementations are not complete capabilities and cannot
/// be selected or advertised through this enum.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CpuFeature {
    /// Eight-byte compare and exchange: CMPXCHG8B.
    Cx8,
    /// Integer conditional moves: CMOVcc. The model does not advertise an FPU.
    Cmov,
    /// Population count: POPCNT, independently of SSE4.2.
    Popcnt,
}

impl CpuFeature {
    const fn mask(self) -> u8 {
        1 << self as u8
    }
}

/// Immutable capability choices for one virtual processor.
///
/// The default enables all complete optional capabilities. Disabling one changes
/// CPUID reporting and instruction availability in blocks and interpreters.
/// Baseline instructions and existing partial x87/SSE implementations remain
/// available; feature selection does not emulate an older processor generation.
///
/// Only implemented, independent capabilities are selectable. There is no raw
/// CPUID mask or constructor that can claim an unimplemented feature group.
/// Keep this model fixed for a guest and include it in compiled-entry cache keys.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CpuModel {
    features: u8,
}

impl Default for CpuModel {
    fn default() -> Self {
        Self {
            features: CpuFeature::Cx8.mask() | CpuFeature::Cmov.mask() | CpuFeature::Popcnt.mask(),
        }
    }
}

impl CpuModel {
    /// Enable or disable one complete optional capability.
    #[must_use]
    pub const fn with_feature(mut self, feature: CpuFeature, enabled: bool) -> Self {
        if enabled {
            self.features |= feature.mask();
        } else {
            self.features &= !feature.mask();
        }
        self
    }

    /// Whether this model provides the selected capability.
    pub const fn supports(self, feature: CpuFeature) -> bool {
        self.features & feature.mask() != 0
    }
}

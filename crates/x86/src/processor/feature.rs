//! Architectural capability names and this build's instruction coverage.

/// A CPUID-enumerated instruction capability within the 32-bit Pentium 4 target.
///
/// The catalog includes Prescott's SSE3 and MONITOR/MWAIT. Platform facilities,
/// 64-bit-only capabilities and later instruction extensions are outside its scope.
/// [`Self::is_implemented`] identifies capabilities with usable instructions;
/// [`Self::is_complete`] identifies those ready for CPUID advertisement.
/// Identifiers are independent of their architectural CPUID bit positions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CpuFeature {
    /// Eight-byte compare and exchange: CMPXCHG8B.
    Cx8,
    /// Conditional moves: CMOVcc, plus FCMOV and FCOMI when an FPU is advertised.
    Cmov,
    /// Timestamp reads: RDTSC. Execution profiles separately govern permission.
    Tsc,
    /// x87 floating-point instructions; FCMOV/FCOMI and FISTTP also need their extensions.
    Fpu,
    /// MMX packed integer instructions and EMMS.
    Mmx,
    /// Floating-point and SIMD context transfers: FXSAVE and FXRSTOR.
    Fxsr,
    /// Streaming SIMD Extensions, including their MMX instruction additions.
    Sse,
    /// Streaming SIMD Extensions 2.
    Sse2,
    /// Streaming SIMD Extensions 3, including x87 FISTTP.
    Sse3,
    /// Cache-line flushing: CLFLUSH, enumerated independently of SSE2.
    Clflush,
    /// Fast system-call transitions: SYSENTER and SYSEXIT.
    Sep,
    /// Address monitoring and waiting: MONITOR and MWAIT, independently of SSE3.
    Monitor,
}

const COMPLETE_FEATURES: u16 = CpuFeature::Cx8.mask()
    | CpuFeature::Cmov.mask()
    | CpuFeature::Tsc.mask()
    | CpuFeature::Fpu.mask()
    | CpuFeature::Mmx.mask()
    | CpuFeature::Fxsr.mask()
    | CpuFeature::Sse.mask()
    | CpuFeature::Sse2.mask()
    | CpuFeature::Clflush.mask();

// FISTTP is usable before the remaining SSE3 instructions are implemented.
pub(super) const IMPLEMENTED_FEATURES: u16 = COMPLETE_FEATURES | CpuFeature::Sse3.mask();

impl CpuFeature {
    pub(super) const fn mask(self) -> u16 {
        1 << self as u8
    }

    /// Whether this build implements instructions from the capability.
    ///
    /// Partially implemented capabilities can be enabled in a [`super::CpuModel`].
    /// This does not promise every instruction or CPUID advertisement.
    pub const fn is_implemented(self) -> bool {
        IMPLEMENTED_FEATURES & self.mask() != 0
    }

    /// Whether this build implements the capability's full advertised behavior.
    ///
    /// CPUID reports a complete capability only when the model also enables it.
    pub const fn is_complete(self) -> bool {
        COMPLETE_FEATURES & self.mask() != 0
    }
}

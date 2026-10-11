//! Architectural capability names and the complete subset implemented by this build.

/// A CPUID-enumerated instruction capability within the 32-bit Pentium 4 target.
///
/// The catalog includes Prescott's SSE3 and MONITOR/MWAIT. Platform facilities,
/// 64-bit-only capabilities and later instruction extensions are outside its scope.
/// Naming a capability does not imply a complete implementation; check
/// [`Self::is_implemented`] before offering it as an enabled model choice.
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
    /// Complete x87 floating-point instruction support.
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

pub(super) const IMPLEMENTED_FEATURES: u16 = CpuFeature::Cx8.mask() | CpuFeature::Cmov.mask();

impl CpuFeature {
    pub(super) const fn mask(self) -> u16 {
        1 << self as u8
    }

    /// Whether this build implements the complete capability for advertisement.
    ///
    /// Partial instruction support does not make an entire feature available.
    /// This is independent of whether a particular [`super::CpuModel`] enables it.
    pub const fn is_implemented(self) -> bool {
        IMPLEMENTED_FEATURES & self.mask() != 0
    }
}

//! Implemented architectural capabilities of the virtual processor.

/// A configurable, fully implemented instruction capability in the Pentium 4 target.
///
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
    /// x87 floating-point instructions; FCMOV/FCOMI also require CMOV.
    Fpu,
    /// MMX packed integer instructions and EMMS.
    Mmx,
    /// Floating-point and SIMD context transfers: FXSAVE and FXRSTOR.
    Fxsr,
    /// Streaming SIMD Extensions, including their MMX instruction additions.
    Sse,
    /// Streaming SIMD Extensions 2.
    Sse2,
    /// Cache-line flushing: CLFLUSH, enumerated independently of SSE2.
    Clflush,
}

pub(super) const ALL_FEATURES: u16 = CpuFeature::Cx8.mask()
    | CpuFeature::Cmov.mask()
    | CpuFeature::Tsc.mask()
    | CpuFeature::Fpu.mask()
    | CpuFeature::Mmx.mask()
    | CpuFeature::Fxsr.mask()
    | CpuFeature::Sse.mask()
    | CpuFeature::Sse2.mask()
    | CpuFeature::Clflush.mask();

impl CpuFeature {
    pub(super) const fn mask(self) -> u16 {
        1 << self as u8
    }
}

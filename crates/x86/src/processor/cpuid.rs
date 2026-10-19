//! Architectural CPUID leaves derived from the selected virtual processor model.

use super::CpuFeature;
use relocade_compiler::{Val, I32};

pub(crate) struct CpuidValues {
    pub(crate) eax: Val<I32>,
    pub(crate) ebx: Val<I32>,
    pub(crate) ecx: Val<I32>,
    pub(crate) edx: Val<I32>,
}

// Family 6, model 0, stepping 1 identifies this virtual model, not an Intel CPU.
const SIGNATURE: u32 = 0x0000_0601;

/// Leaves 0 and 1 have no subleaves. Unsupported queries return the highest
/// basic leaf, including extended queries because there is no extended range.
pub(crate) fn cpuid(features: &Val<I32>, leaf: &Val<I32>) -> CpuidValues {
    let reports = |feature: CpuFeature| features.and(feature.mask()).ne(0).select(1u32, 0u32);
    let edx = reports(CpuFeature::Fpu)
        .or(reports(CpuFeature::Tsc).shl(4))
        .or(reports(CpuFeature::Cx8).shl(8))
        .or(reports(CpuFeature::Cmov).shl(15))
        .or(reports(CpuFeature::Clflush).shl(19))
        .or(reports(CpuFeature::Mmx).shl(23))
        .or(reports(CpuFeature::Fxsr).shl(24))
        .or(reports(CpuFeature::Sse).shl(25))
        .or(reports(CpuFeature::Sse2).shl(26));
    // The virtual CLFLUSH line is 64 bytes, encoded in eight-byte units.
    let ebx = reports(CpuFeature::Clflush).shl(11);
    let vendor = leaf.eq(0);
    CpuidValues {
        eax: vendor.select(1, SIGNATURE),
        ebx: vendor.select(u32::from_le_bytes(*b"Relo"), ebx),
        ecx: vendor.select(u32::from_le_bytes(*b" CPU"), 0),
        edx: vendor.select(u32::from_le_bytes(*b"cade"), edx),
    }
}

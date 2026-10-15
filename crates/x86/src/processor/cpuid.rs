//! Architectural CPUID leaves derived from the selected virtual processor model.

use super::{CpuFeature, CpuModel};
use wasm86_compiler::{Val, I32};

pub(crate) struct CpuidValues {
    pub(crate) eax: Val<I32>,
    pub(crate) ebx: Val<I32>,
    pub(crate) ecx: Val<I32>,
    pub(crate) edx: Val<I32>,
}

// Family 6, model 0, stepping 1 identifies this virtual model, not an Intel CPU.
const SIGNATURE: u32 = 0x0000_0601;

impl CpuModel {
    /// Leaves 0 and 1 have no subleaves. Unsupported queries return the highest
    /// basic leaf, including extended queries because there is no extended range.
    pub(crate) fn cpuid(self, leaf: &Val<I32>) -> CpuidValues {
        let reports = |feature: CpuFeature| u32::from(self.supports(feature));
        let edx = reports(CpuFeature::Fpu)
            | (reports(CpuFeature::Tsc) << 4)
            | (reports(CpuFeature::Cx8) << 8)
            | (reports(CpuFeature::Cmov) << 15)
            | (reports(CpuFeature::Clflush) << 19)
            | (reports(CpuFeature::Mmx) << 23)
            | (reports(CpuFeature::Fxsr) << 24)
            | (reports(CpuFeature::Sse) << 25)
            | (reports(CpuFeature::Sse2) << 26);
        // The virtual CLFLUSH line is 64 bytes, encoded in eight-byte units.
        let ebx = reports(CpuFeature::Clflush) * (8 << 8);
        let vendor = leaf.eq(0);
        CpuidValues {
            eax: vendor.select(1, SIGNATURE),
            ebx: vendor.select(u32::from_le_bytes(*b"Relo"), ebx),
            ecx: vendor.select(u32::from_le_bytes(*b" CPU"), 0),
            edx: vendor.select(u32::from_le_bytes(*b"cade"), edx),
        }
    }
}

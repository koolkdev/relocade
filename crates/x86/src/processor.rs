//! The built-in virtual CPU identity and complete advertised instruction features.
//!
//! Execution profiles change decoding and memory assumptions, not this identity.

use wasm86_compiler::{Val, I32};

pub(crate) struct CpuidValues {
    pub(crate) eax: Val<I32>,
    pub(crate) ebx: Val<I32>,
    pub(crate) ecx: Val<I32>,
    pub(crate) edx: Val<I32>,
}

// The virtual model uses family 6, model 0, stepping 1 without claiming an Intel
// processor identity. Feature flags describe the implementation independently.
const SIGNATURE: u32 = 0x0000_0601;
const POPCNT: u32 = 1 << 23;
const CX8: u32 = 1 << 8;
const CMOV: u32 = 1 << 15;

/// Leaves 0 and 1 have no subleaves. All unsupported queries return the highest
/// basic leaf, including extended queries because this model has no extended range.
pub(crate) fn cpuid(leaf: &Val<I32>) -> CpuidValues {
    let vendor = leaf.eq(0);
    CpuidValues {
        eax: vendor.select(1, SIGNATURE),
        ebx: vendor.select(u32::from_le_bytes(*b"Relo"), 0),
        ecx: vendor.select(u32::from_le_bytes(*b" CPU"), POPCNT),
        edx: vendor.select(u32::from_le_bytes(*b"cade"), CX8 | CMOV),
    }
}

//! Optional inputs for locally guarded specialization, independent of CPU layout.

use crate::CpuState;

/// Observed values are hints, not entry requirements. Consumers guard the current
/// value before using a hint; missing fields stay dynamic.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuObservations {
    pub x87: X87Observations,
    /// Only MXCSR control bits participate in numerical specialization.
    pub mxcsr: Option<u32>,
}

/// Independent x87 observations. PC and RC use their low two architectural bits.
/// Arithmetic can use PM and PE together when both are known set. Rounded stores
/// consume RC alone, leaving all other controls and status live.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct X87Observations {
    pub precision_control: Option<u8>,
    pub rounding_control: Option<u8>,
    pub precision_mask: Option<bool>,
    pub precision_raised: Option<bool>,
}

impl CpuObservations {
    /// Extracts only fields supported by guarded specialization.
    pub fn from_cpu(cpu: &CpuState) -> Self {
        Self {
            x87: X87Observations {
                precision_control: Some(cpu.x87.control.precision_control & 3),
                rounding_control: Some(cpu.x87.control.rounding_control & 3),
                precision_mask: Some(cpu.x87.control.precision_mask & 1 != 0),
                precision_raised: Some(cpu.x87.status.precision & 1 != 0),
            },
            mxcsr: Some(cpu.simd.mxcsr),
        }
    }
}

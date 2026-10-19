//! Admission shared by block boundaries and instruction execution.

use crate::{CpuFeature, Exception, ExecutionProfile, ExecutionTarget};
use relocade_compiler::{Val, I32};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Availability {
    AllModes,
    ProtectedOnly,
    /// Requires CPL <= IOPL; supported protected profiles fix CPL3/IOPL0.
    IoPrivileged,
}

/// Checked after complete decoding, before operand access or instruction effects.
#[derive(Clone, Copy)]
pub(super) struct Requirements {
    pub(super) availability: Availability,
    pub(super) features: &'static [CpuFeature],
}

impl Requirements {
    pub(super) fn fault(self, target: ExecutionTarget) -> Option<Exception<Val<I32>>> {
        if self
            .features
            .iter()
            .any(|&feature| !target.cpu_model.supports(feature))
        {
            return Some(Exception::InvalidOpcode);
        }
        match (self.availability, target.profile) {
            (Availability::ProtectedOnly, ExecutionProfile::Real16) => {
                Some(Exception::InvalidOpcode)
            }
            (Availability::IoPrivileged, ExecutionProfile::Protected(_)) => {
                Some(Exception::GeneralProtection {
                    error_code: 0.into(),
                })
            }
            _ => None,
        }
    }
}

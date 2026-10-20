//! Admission shared by block boundaries and instruction execution.

use crate::{processor, CpuFeature, Exception, ExecutionProfile, ExecutionTarget};
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
        if processor::supports_features(target.cpu_model.bits(), self.features).constant_bits()
            == Some(0)
        {
            return Some(Exception::InvalidOpcode);
        }
        self.mode_fault(target.profile)
    }

    pub(super) fn mode_fault(self, profile: ExecutionProfile) -> Option<Exception<Val<I32>>> {
        match (self.availability, profile) {
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

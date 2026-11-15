//! Admission shared by block boundaries and instruction execution.

use crate::{
    execution::ExecutionBuilder, processor, CpuFeature, Exception, ExecutionProfile,
    ExecutionTarget, PrivilegeLevel,
};
use relocade_compiler::BuildError;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Availability {
    AllModes,
    ProtectedOnly,
    /// Requires CPL0.
    Privileged,
    /// Requires CPL <= the execution policy's effective IOPL.
    IoPrivileged,
    /// CR4.TSD restricts RDTSC to CPL0.
    TimestampCounter,
}

/// Checked after complete decoding, before operand access or instruction effects.
#[derive(Clone, Copy)]
pub(super) struct Requirements {
    pub(super) availability: Availability,
    pub(super) features: &'static [CpuFeature],
}

impl Requirements {
    pub(super) fn check(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        // Missing CPU features raise #UD before state-based privilege checks.
        let supported = execution.supports_cpu_features(self.features);
        execution.fault_if(supported.eq(false), Exception::InvalidOpcode)?;
        match self.availability {
            Availability::AllModes => Ok(()),
            Availability::ProtectedOnly => {
                let protected = execution.protected_mode()?;
                execution.fault_if(protected.eq(false), Exception::InvalidOpcode)
            }
            Availability::Privileged => {
                let cpl = execution.privilege_level()?;
                execution.fault_if(
                    cpl.ne(PrivilegeLevel::Ring0 as u32),
                    Exception::GeneralProtection { error_code: 0 },
                )
            }
            Availability::IoPrivileged => {
                let denied = execution.io_privilege_denied()?;
                execution.fault_if(denied, Exception::GeneralProtection { error_code: 0 })
            }
            Availability::TimestampCounter => {
                let denied = execution.timestamp_access_denied()?;
                execution.fault_if(denied, Exception::GeneralProtection { error_code: 0 })
            }
        }
    }

    /// A fault established by the captured target can end snapshot discovery.
    pub(super) fn known_fault(self, target: ExecutionTarget) -> Option<Exception> {
        if processor::supports_features(target.cpu_model.bits(), self.features)
            .constant_bits()
            .expect("CPU feature masks are constants")
            == Some(0)
        {
            return Some(Exception::InvalidOpcode);
        }
        self.mode_fault(target.profile)
    }

    /// Snapshot discovery can stop at a fault established by its entry profile.
    /// Execution lowers the corresponding predicates from CPU state values.
    fn mode_fault(self, profile: ExecutionProfile) -> Option<Exception> {
        match (self.availability, profile) {
            (Availability::ProtectedOnly, ExecutionProfile::Real16) => {
                Some(Exception::InvalidOpcode)
            }
            (
                Availability::IoPrivileged | Availability::Privileged,
                ExecutionProfile::Protected(_),
            ) => Some(Exception::GeneralProtection { error_code: 0 }),
            _ => None,
        }
    }
}

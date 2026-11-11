//! Admission shared by block boundaries and instruction execution.

use crate::{
    processor, profile::ProfileFacts, CpuFeature, CpuMode, CpuModel, Exception, PrivilegeLevel,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Availability {
    AllModes,
    ProtectedOnly,
    /// Requires CPL0; supported protected profiles execute at CPL3.
    Privileged,
    /// Requires CPL <= IOPL; protected profiles use CPL3 and effective IOPL0.
    IoPrivileged,
    /// CR4.TSD restricts RDTSC to CPL0 when set.
    TimestampCounter,
}

/// Checked after complete decoding, before operand access or instruction effects.
#[derive(Clone, Copy)]
pub(super) struct Requirements {
    pub(super) availability: Availability,
    pub(super) features: &'static [CpuFeature],
}

impl Requirements {
    pub(super) fn fault(self, cpu_model: CpuModel, facts: &ProfileFacts) -> Option<Exception> {
        if processor::supports_features(cpu_model.bits(), self.features)
            .constant_bits()
            .expect("CPU feature masks are constants")
            == Some(0)
        {
            return Some(Exception::InvalidOpcode);
        }
        self.state_fault(facts)
    }

    pub(super) fn state_fault(self, facts: &ProfileFacts) -> Option<Exception> {
        let denied = match self.availability {
            Availability::AllModes => return None,
            Availability::ProtectedOnly => {
                return (facts.mode() == CpuMode::Real).then_some(Exception::InvalidOpcode);
            }
            Availability::Privileged => facts.privilege_level() != PrivilegeLevel::Ring0,
            Availability::IoPrivileged => !facts.allows_io(),
            Availability::TimestampCounter => {
                facts.timestamp_restricted() && facts.privilege_level() != PrivilegeLevel::Ring0
            }
        };
        denied.then_some(Exception::GeneralProtection { error_code: 0 })
    }
}

//! Admission shared by block boundaries and instruction execution.

use crate::{
    entry::control_assumptions,
    execution::ExecutionBuilder,
    state::{access::cpu_location, cr0},
    CompilerConfig, CpuFeature, CpuMode, EntryAssumptions, Exception, FlagBytes, PrivilegeLevel,
};
use relocade_compiler::{BuildError, Val, I1, I8};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Availability {
    AllModes,
    ProtectedOnly,
    /// Requires CPL0.
    Privileged,
    /// Requires CPL <= IOPL.
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
                    cpl_exceeds(cpl, PrivilegeLevel::Ring0 as u32),
                    Exception::GeneralProtection { error_code: 0 },
                )
            }
            Availability::IoPrivileged => {
                let cpl = execution.privilege_level()?;
                let denied = cpl_exceeds(cpl, execution.read_iopl()?);
                execution.fault_if(denied, Exception::GeneralProtection { error_code: 0 })
            }
            Availability::TimestampCounter => {
                let denied = execution.timestamp_access_denied()?;
                execution.fault_if(denied, Exception::GeneralProtection { error_code: 0 })
            }
        }
    }

    /// A snapshot ends at a privilege fault only when its entry guarantees
    /// establish every operand of the check. Otherwise execution decides.
    pub(super) fn always_faults(self, config: CompilerConfig) -> bool {
        self.features
            .iter()
            .any(|&feature| !config.cpu_model.supports(feature))
            || self
                .guaranteed_privilege_fault(config.entry)
                .unwrap_or(false)
    }

    fn guaranteed_privilege_fault(self, entry: EntryAssumptions) -> Option<bool> {
        let facts = || control_assumptions(entry);
        let mode = || {
            let pe = cpu_location!(cr0).known_bits(facts(), cr0::PROTECTION_ENABLE)?;
            let vm = cpu_location!(flags.bytes.vm).known_bits(facts(), FlagBytes::BOOLEAN_MASK)?;
            Some(CpuMode::from_control(pe, vm != 0))
        };
        let limit = match self.availability {
            Availability::AllModes | Availability::TimestampCounter => return None,
            Availability::ProtectedOnly => return Some(mode()? != CpuMode::Protected),
            Availability::Privileged => PrivilegeLevel::Ring0 as u32,
            Availability::IoPrivileged => {
                cpu_location!(flags.bytes.iopl).known_bits(facts(), PrivilegeLevel::MASK)?
            }
        };
        let mode = mode()?;
        let selector = match mode {
            CpuMode::Protected => cpu_location!(segments.cs.selector)
                .known_bits(facts(), PrivilegeLevel::MASK)?
                as u16,
            _ => 0, // The selector does not contribute to CPL outside protected mode.
        };
        let cpl = mode.privilege_level(selector) as u32;
        Some(
            cpl_exceeds(cpl, limit)
                .constant_bits()
                .expect("constant privilege levels")
                == Some(1),
        )
    }
}

fn cpl_exceeds(cpl: impl Into<Val<I8>>, limit: impl Into<Val<I8>>) -> Val<I1> {
    limit.into().unsigned().lt(cpl)
}

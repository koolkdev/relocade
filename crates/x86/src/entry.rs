//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_config::{EntryAssumptions, ExecutionMode, MemoryBackend, SegmentAssumptions};
use std::sync::Arc;

use crate::{
    state::{access::cpu_location, cr0, cr4, StatePattern},
    CpuMode, CpuState, FlagBytes, PrivilegeLevel,
};

/// Bits consumed by mode and privilege predicates. Other supported-state
/// restrictions remain in CPU admission until their transitions are implemented.
pub(crate) fn control_assumptions(mode: ExecutionMode) -> Vec<StatePattern> {
    macro_rules! require_bits {
        ($($field:ident).+, $mask:expr, $expected:expr) => {
            StatePattern::bits(
                cpu_location!($($field).+),
                u64::from($mask),
                u64::from($expected),
            )
        };
    }
    let protected = mode == ExecutionMode::ProtectedUser;
    let mut requirements = vec![
        require_bits!(
            cr0,
            cr0::PROTECTION_ENABLE,
            if protected { cr0::PROTECTION_ENABLE } else { 0 }
        ),
        require_bits!(flags.bytes.vm, FlagBytes::BOOLEAN_MASK, 0u8),
        require_bits!(cr4, cr4::TIMESTAMP_DISABLE, 0u32),
    ];
    if protected {
        requirements.push(require_bits!(
            segments.cs.selector,
            u16::from(PrivilegeLevel::MASK),
            PrivilegeLevel::Ring3 as u16
        ));
    }
    requirements
}

/// CPU field values the execution owner guarantees before entering the module.
/// Builders may fold reads from these facts before emitting any guard.
#[derive(Clone)]
pub(crate) struct EntryGuarantees {
    fields: Arc<[StatePattern]>,
}

impl EntryGuarantees {
    pub(crate) fn from_assumptions(entry: EntryAssumptions) -> Self {
        let mut fields = control_assumptions(entry.mode);
        for segment in crate::Segment::ALL {
            fields.extend(crate::segment::assumptions::field_patterns(entry, segment));
        }
        Self {
            fields: fields.into(),
        }
    }

    pub(crate) fn patterns(&self) -> &[StatePattern] {
        &self.fields
    }
}

impl CpuState {
    /// Checks the guaranteed entry facts and supported control bits. Values
    /// outside the supported baseline can be stored but cannot execute yet.
    /// This check does not reload segments or authorize mode transitions.
    pub fn is_compatible_with(&self, entry: EntryAssumptions) -> bool {
        let mode = match entry.mode {
            ExecutionMode::Real => CpuMode::Real,
            ExecutionMode::ProtectedUser => CpuMode::Protected,
        };
        self.mode() == mode
            && self.flags.bytes.vm & FlagBytes::BOOLEAN_MASK == 0
            // ET and NE are set; EM, TS, paging and other unimplemented controls are clear.
            && self.cr0 & !cr0::PROTECTION_ENABLE == (cr0::EXTENSION_TYPE | cr0::NUMERIC_ERROR)
            // OSFXSR and OSXMMEXCPT are set; TSD and other controls are clear.
            && self.cr4 == (cr4::OS_FXSR | cr4::OS_XMM_EXCEPTIONS)
            && self.segments.is_compatible_with(entry)
    }
}

#[cfg(test)]
mod tests;

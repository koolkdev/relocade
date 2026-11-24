//! Architectural entry facts used for CPU admission and state specialization.

pub use relocade_x86_config::{EntryAssumptions, ExecutionMode, MemoryBackend, SegmentAssumptions};
use std::{mem::offset_of, sync::Arc};

use crate::{
    state::{access::cpu_location, cr0, cr4, StatePattern},
    CpuState, ExecutionContext, FlagBytes, PrivilegeLevel,
};

macro_rules! require_bits {
    ($($field:ident).+, $mask:expr, $expected:expr) => {
        StatePattern::bits(
            cpu_location!($($field).+),
            u64::from($mask),
            u64::from($expected),
        )
    };
}

/// Control bits established by the runtime and available to generated code on entry.
pub(crate) fn control_assumptions(
    entry: EntryAssumptions,
) -> impl Iterator<Item = StatePattern> + Clone {
    [
        entry.mode.map(|mode| {
            require_bits!(
                cr0,
                cr0::PROTECTION_ENABLE,
                if mode == ExecutionMode::ProtectedUser {
                    cr0::PROTECTION_ENABLE
                } else {
                    0
                }
            )
        }),
        entry
            .cr4_tsd_clear
            .then(|| require_bits!(cr4, cr4::TIMESTAMP_DISABLE, 0u32)),
        entry
            .mode
            .map(|_| require_bits!(flags.bytes.vm, FlagBytes::BOOLEAN_MASK, 0u8)),
        (entry.mode == Some(ExecutionMode::ProtectedUser)).then(|| {
            require_bits!(
                segments.cs.selector,
                u16::from(PrivilegeLevel::MASK),
                PrivilegeLevel::Ring3 as u16
            )
        }),
        entry.interrupt_flag.map(|enabled| {
            require_bits!(flags.bytes.if_, FlagBytes::BOOLEAN_MASK, u8::from(enabled))
        }),
        entry
            .iopl
            .map(|level| require_bits!(flags.bytes.iopl, PrivilegeLevel::MASK, level as u8)),
    ]
    .into_iter()
    .flatten()
}

/// CPU field values the execution owner establishes on every entry to the module.
/// Builders may fold reads from these facts before emitting any guard.
#[derive(Clone)]
pub(crate) struct EntryGuarantees(Arc<[StatePattern]>);

impl EntryGuarantees {
    pub(crate) fn for_entry(entry: EntryAssumptions) -> Self {
        let mut fields: Vec<_> = control_assumptions(entry).collect();
        for segment in crate::Segment::ALL {
            fields.extend(crate::segment::assumptions::field_patterns(entry, segment));
        }
        Self(fields.into())
    }

    pub(crate) fn patterns(&self) -> &[StatePattern] {
        &self.0
    }
}

pub(crate) fn matches_control_write(entry: EntryAssumptions, offset: usize, bytes: &[u8]) -> bool {
    if offset > CpuState::BYTE_LEN || bytes.len() > CpuState::BYTE_LEN - offset {
        return false;
    }
    let offset = offset_of!(ExecutionContext, cpu) + offset;
    control_assumptions(entry).all(|field| field.matches_record(bytes, offset))
}

impl CpuState {
    /// Checks the supported control state and the supplied architectural entry facts.
    pub fn is_compatible_with(&self, entry: EntryAssumptions) -> bool {
        self.has_supported_control_state()
            && matches_control_write(entry, 0, &self.to_bytes())
            && self.segments.is_compatible_with(entry)
    }

    /// Checks the implemented CR0 and CR4 bits and requires virtual-8086 mode to be off.
    pub fn has_supported_control_state(&self) -> bool {
        // ET and NE are set. EM, TS, paging and other unimplemented controls are clear.
        self.cr0 & !cr0::PROTECTION_ENABLE == (cr0::EXTENSION_TYPE | cr0::NUMERIC_ERROR)
            // OSFXSR and OSXMMEXCPT are set. TSD and other controls are clear.
            && self.cr4 == (cr4::OS_FXSR | cr4::OS_XMM_EXCEPTIONS)
            && self.flags.bytes.vm & FlagBytes::BOOLEAN_MASK == 0
    }
}

#[cfg(test)]
mod tests;

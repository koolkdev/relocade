//! Execution mode and assumptions fixed when an entry is compiled.

pub use relocade_x86_profile::{
    EntryAssumptions, ExecutionMode, ExecutionProfile, MemoryBackend, SegmentAssumptions,
};
use std::{mem::offset_of, sync::Arc};

use crate::{
    state::{access::cpu_location, cr0, cr4, StatePattern},
    CpuModel, CpuState, ExecutionContext, FlagBytes, PrivilegeLevel,
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

/// CPU field values the execution owner establishes on every entry to this profile.
/// Builders may fold reads from these facts before emitting any guard.
#[derive(Clone)]
pub(crate) struct EntryGuarantees(Arc<[StatePattern]>);

impl EntryGuarantees {
    pub(crate) fn for_entry(entry: EntryAssumptions) -> Self {
        let mut fields: Vec<_> = control_assumptions(entry).collect();
        for segment in crate::Segment::ALL {
            fields.extend(crate::segment::profile::assumptions(entry, segment));
        }
        Self(fields.into())
    }

    pub(crate) fn patterns(&self) -> &[StatePattern] {
        &self.0
    }
}

/// The entry facts and code-generation settings required by a compiled artifact.
/// Include the complete configuration in cache and direct-link keys.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CompilerConfig {
    pub entry: EntryAssumptions,
    /// Memory routing and imports used for instruction fetch and data accesses.
    pub memory_backend: MemoryBackend,
    pub cpu_model: CpuModel,
    /// Disabled budgets require the owner to keep the live budget zero.
    pub instruction_budgets: bool,
}

impl CompilerConfig {
    /// Uses the default processor model and enables instruction budgets.
    pub const fn new(entry: EntryAssumptions, memory_backend: MemoryBackend) -> Self {
        Self {
            entry,
            memory_backend,
            cpu_model: CpuModel::DEFAULT,
            instruction_budgets: true,
        }
    }

    /// Checks constrained control bits touched by a host write relative to `CpuState`.
    pub fn matches_control_write(self, offset: usize, bytes: &[u8]) -> bool {
        matches_control_write(self.entry, offset, bytes)
    }

    /// Checks the artifact configuration and current CPU image. Code bytes and
    /// instruction-fetch mappings require separate validation.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && cpu.is_compatible_with(self.entry)
    }
}

fn matches_control_write(entry: EntryAssumptions, offset: usize, bytes: &[u8]) -> bool {
    if offset > CpuState::BYTE_LEN || bytes.len() > CpuState::BYTE_LEN - offset {
        return false;
    }
    let offset = offset_of!(ExecutionContext, cpu) + offset;
    control_assumptions(entry).all(|field| field.matches_record(bytes, offset))
}

impl From<CompilerConfig> for EntryAssumptions {
    fn from(config: CompilerConfig) -> Self {
        config.entry
    }
}

impl From<ExecutionTarget> for EntryAssumptions {
    fn from(target: ExecutionTarget) -> Self {
        let mut entry = Self::from(target.profile);
        if target.memory_backend == MemoryBackend::PhysicalBus {
            entry.interrupt_flag = None;
            entry.iopl = None;
        }
        entry
    }
}

impl From<ExecutionTarget> for CompilerConfig {
    fn from(target: ExecutionTarget) -> Self {
        Self {
            entry: target.into(),
            memory_backend: target.memory_backend,
            cpu_model: target.cpu_model,
            instruction_budgets: target.instruction_budgets,
        }
    }
}

impl<P: Into<ExecutionProfile>> From<P> for CompilerConfig {
    fn from(profile: P) -> Self {
        ExecutionTarget::from(profile).into()
    }
}

/// The processor model and execution assumptions required by a compiled entry.
///
/// Use the complete value in cache and direct-link keys. Blocks and their fallback
/// interpreters must agree on this target, including disabled capabilities.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionTarget {
    pub profile: ExecutionProfile,
    /// Memory routing and imports required by instruction fetch and data accesses.
    pub memory_backend: MemoryBackend,
    pub cpu_model: CpuModel,
    /// Whether the execution owner may arm instruction budgets. When false,
    /// snapshot blocks omit their budget checks and the live budget must stay zero.
    pub instruction_budgets: bool,
}

impl ExecutionTarget {
    pub(crate) fn new(profile: ExecutionProfile, memory_backend: MemoryBackend) -> Self {
        Self {
            profile,
            memory_backend,
            cpu_model: CpuModel::default(),
            instruction_budgets: true,
        }
    }

    /// Checks the constrained control bits touched by a host write to the CPU image.
    /// The offset is relative to `CpuState`. Bits outside the write keep their live values.
    pub fn matches_control_write(self, offset: usize, bytes: &[u8]) -> bool {
        CompilerConfig::from(self).matches_control_write(offset, bytes)
    }

    /// Check an entry against the installed target and architectural CPU state.
    ///
    /// The CPU image supplies the mode; the target retains generated-code policy
    /// and specialization requirements. Code bytes, fetch permissions and mappings require
    /// separate validation as described by the entry-validity contract.
    pub fn is_compatible_with(self, active: Self, cpu: &CpuState) -> bool {
        self == active && cpu.is_compatible_with(self)
    }
}

impl<P: Into<ExecutionProfile>> From<P> for ExecutionTarget {
    fn from(profile: P) -> Self {
        let profile = profile.into();
        Self::new(profile, MemoryBackend::default_for(profile))
    }
}

impl CpuState {
    /// Checks the supported control state and the supplied architectural entry facts.
    pub fn is_compatible_with(&self, entry: impl Into<EntryAssumptions>) -> bool {
        let entry = entry.into();
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

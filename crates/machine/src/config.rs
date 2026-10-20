use relocade_x86::{
    CpuState, DescriptorTableRegister, ExecutionProfile, ExecutionTarget, PhysicalMemoryMap,
    SystemSegmentRegister,
};
use std::fmt;

/// Validated initial CPU state, backing capacity and physical mappings.
pub struct MachineConfig {
    target: ExecutionTarget,
    cpu: CpuState,
    memory_size: u64,
    map: PhysicalMemoryMap,
}

impl MachineConfig {
    /// Configures a real-mode guest without allocating its backing.
    /// All RAM/ROM pages must fit `memory_size`, which can be at most 4 GiB.
    pub fn new(
        target: ExecutionTarget,
        cpu: CpuState,
        memory_size: u64,
        map: PhysicalMemoryMap,
    ) -> Result<Self, ConfigurationError> {
        if target.profile != ExecutionProfile::Real16 {
            return Err(ConfigurationError::UnsupportedProfile);
        }
        validate_cpu(target, &cpu)?;
        if memory_size > 1 << 32 {
            return Err(ConfigurationError::InvalidMemorySize);
        }
        if !map.fits_backing(memory_size) {
            return Err(ConfigurationError::InsufficientMemory);
        }
        Ok(Self {
            target,
            cpu,
            memory_size,
            map,
        })
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.target
    }

    pub fn cpu(&self) -> &CpuState {
        &self.cpu
    }

    /// Requested initial backing capacity in bytes.
    pub fn memory_size(&self) -> u64 {
        self.memory_size
    }

    pub fn memory_map(&self) -> &PhysicalMemoryMap {
        &self.map
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    UnsupportedProfile,
    IncompatibleCpu,
    InvalidMemorySize,
    InsufficientMemory,
    InvalidTableRegister,
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTableRegister => {
                "CPU table registers do not satisfy the execution profile"
            }
            Self::UnsupportedProfile => "physical machine configuration requires real mode",
            Self::IncompatibleCpu => "CPU segment caches do not satisfy the execution target",
            Self::InvalidMemorySize => "memory_size exceeds the 4-GiB backing address space",
            Self::InsufficientMemory => "physical mappings exceed memory_size",
        })
    }
}

impl std::error::Error for ConfigurationError {}

pub(crate) fn validate_cpu(
    target: ExecutionTarget,
    cpu: &CpuState,
) -> Result<(), ConfigurationError> {
    if !target.profile.is_compatible_with(&cpu.segments) {
        return Err(ConfigurationError::IncompatibleCpu);
    }
    validate_idtr(target.profile, cpu.idtr)?;
    validate_system_segment(cpu.ldtr, true)?;
    validate_system_segment(cpu.tr, false)
}

pub(crate) fn validate_idtr(
    profile: ExecutionProfile,
    idtr: DescriptorTableRegister,
) -> Result<(), ConfigurationError> {
    if profile == ExecutionProfile::Real16 && (idtr.base != 0 || idtr.limit != 0x3ff) {
        return Err(ConfigurationError::InvalidTableRegister);
    }
    Ok(())
}

pub(crate) fn validate_system_segment(
    segment: SystemSegmentRegister,
    ldt: bool,
) -> Result<(), ConfigurationError> {
    if segment.usable == 0 {
        return Ok(());
    }
    let kind = (segment.access_rights >> 8) & 15;
    let valid_type = if ldt {
        kind == 2
    } else {
        matches!(kind, 1 | 3 | 9 | 11)
    };
    if segment.selector & !3 == 0
        || segment.selector & 4 != 0
        || segment.access_rights & 0x9000 != 0x8000
        || !valid_type
    {
        return Err(ConfigurationError::InvalidTableRegister);
    }
    Ok(())
}

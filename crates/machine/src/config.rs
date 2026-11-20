mod cpu;
mod machine;

pub use cpu::CpuConfig;
pub(crate) use cpu::{validate_cpu, validate_idtr, validate_system_segment};
pub use machine::MachineConfig;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    UnsupportedMode,
    UnsupportedMemoryBackend,
    IncompatibleCpu,
    IncompatibleIoBus,
    GuestTimeRequiresInstructionBudgets,
    CpuAlreadyAttached,
    InvalidMemorySize,
    UnalignedMemorySize,
    InsufficientMemory,
    InvalidTableRegister,
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTableRegister => {
                "CPU table registers do not satisfy the execution profile"
            }
            Self::UnsupportedMode => {
                "execution profile is incompatible with the memory configuration"
            }
            Self::UnsupportedMemoryBackend => {
                "memory backend is incompatible with the memory configuration"
            }
            Self::CpuAlreadyAttached => "a guest CPU is already attached to this PC",
            Self::GuestTimeRequiresInstructionBudgets => {
                "guest time requires CPU instruction budgets"
            }
            Self::IncompatibleIoBus => "a PC CPU requires a device bus and a flat CPU takes none",
            Self::IncompatibleCpu => "CPU state does not satisfy the execution target",
            Self::InvalidMemorySize => "memory_size exceeds the 4-GiB backing address space",
            Self::UnalignedMemorySize => "flat memory_size must be a multiple of 4 KiB",
            Self::InsufficientMemory => "physical mappings exceed memory_size",
        })
    }
}

impl std::error::Error for ConfigurationError {}

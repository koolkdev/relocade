mod cpu;
mod machine;

pub use cpu::CpuConfig;
pub(crate) use cpu::{validate_cpu, validate_idtr, validate_system_segment};
pub use machine::MachineConfig;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    UnsupportedProfile,
    IncompatibleCpu,
    InvalidMemorySize,
    InsufficientMemory,
    InvalidTableRegister,
    InterruptsRequireRealMode,
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InterruptsRequireRealMode => "external interrupt delivery requires real mode",
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

mod cpu;
mod machine;

pub use cpu::CpuConfig;
pub(crate) use cpu::{validate_cpu, validate_idtr, validate_system_segment};
pub use machine::MachineConfig;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    MemoryMapMismatch,
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
            Self::MemoryMapMismatch => "memory map does not match the execution profile",
            Self::IncompatibleCpu => "CPU segment caches do not satisfy the execution target",
            Self::InvalidMemorySize => "memory_size exceeds the 4-GiB backing address space",
            Self::InsufficientMemory => "memory mappings exceed memory_size",
        })
    }
}

impl std::error::Error for ConfigurationError {}

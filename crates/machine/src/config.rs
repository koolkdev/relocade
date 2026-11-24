mod cpu;
mod machine;

pub use cpu::CpuConfig;
pub(crate) use cpu::{validate_idtr, validate_system_segment, CpuExecution};
pub use machine::MachineConfig;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    UnsupportedEntryAssumptions,
    UnsupportedMemoryBackend,
    IncompatibleCpu,
    GuestTimeRequiresInstructionBudgets,
    CpuAlreadyAttached,
    InvalidMemorySize,
    InsufficientMemory,
    InvalidTableRegister,
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTableRegister => "unsupported CPU table registers",
            Self::UnsupportedEntryAssumptions => {
                "the physical PC JIT requires canonical real-mode entry assumptions"
            }
            Self::UnsupportedMemoryBackend => {
                "physical machine configuration requires the physical memory bus"
            }
            Self::CpuAlreadyAttached => "a guest CPU is already attached to this PC",
            Self::GuestTimeRequiresInstructionBudgets => {
                "guest time requires CPU instruction budgets"
            }
            Self::IncompatibleCpu => "unsupported CPU control state or segment caches",
            Self::InvalidMemorySize => "memory_size exceeds the 4-GiB backing address space",
            Self::InsufficientMemory => "physical mappings exceed memory_size",
        })
    }
}

impl std::error::Error for ConfigurationError {}

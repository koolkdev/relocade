use std::fmt;
use wasm86_x86::{CpuState, ExecutionTarget};

/// An architectural snapshot and the explicit mode/model under which it runs.
/// Cache contents cannot establish real versus protected mode on their own.
#[derive(Clone, Copy, Debug)]
pub struct CpuContext {
    target: ExecutionTarget,
    cpu: CpuState,
}

impl CpuContext {
    pub fn new(target: ExecutionTarget, cpu: CpuState) -> Result<Self, ConfigurationError> {
        if !target.profile.is_compatible_with(&cpu.segments) {
            return Err(ConfigurationError::IncompatibleCpu);
        }
        Ok(Self { target, cpu })
    }
    pub fn target(self) -> ExecutionTarget {
        self.target
    }
    pub fn cpu(self) -> CpuState {
        self.cpu
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationError {
    IncompatibleCpu,
    IncompatibleMemory,
}
impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IncompatibleCpu => "CPU segment caches do not satisfy the execution target",
            Self::IncompatibleMemory => "address-space model does not match the execution mode",
        })
    }
}
impl std::error::Error for ConfigurationError {}

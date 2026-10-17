//! Guest configuration, address spaces and environment services, independent of
//! the Wasm engine. Attaching a machine consumes its initial CPU image; live CPU
//! and RAM backing then belong to the execution instance.
#![forbid(unsafe_code)]

mod environment;
mod imports;
mod memory;

pub use environment::Environment;
pub use imports::Imports;
pub use memory::{GuestMemory, MemoryAccess, MemoryError, MemoryMap};
use std::fmt;
use wasm86_x86::{CpuState, ExecutionTarget};

/// A configured guest ready to attach to an execution runtime.
pub struct Machine<E> {
    target: ExecutionTarget,
    cpu: CpuState,
    imports: Imports<E>,
}

impl<E: Environment> Machine<E> {
    pub fn new(
        target: ExecutionTarget,
        cpu: CpuState,
        map: impl Into<MemoryMap>,
        environment: E,
    ) -> Result<Self, ConfigurationError> {
        if !target.profile.is_compatible_with(&cpu.segments) {
            return Err(ConfigurationError::IncompatibleCpu);
        }
        let map = map.into();
        if !map.supports(target.profile) {
            return Err(ConfigurationError::IncompatibleMemory);
        }
        Ok(Self {
            target,
            cpu,
            imports: Imports::new(map, environment),
        })
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.target
    }

    /// Supplies the runtime's initial CPU image and persistent environment state.
    pub fn into_parts(self) -> (CpuState, Imports<E>) {
        (self.cpu, self.imports)
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
            Self::IncompatibleMemory => "address-space model does not match the execution profile",
        })
    }
}

impl std::error::Error for ConfigurationError {}

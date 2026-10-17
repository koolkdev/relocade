//! Initial machine configuration, independent of the execution backend.
//!
//! A configuration supplies a real-mode CPU and a fixed physical memory map.
//! `memory_size` specifies backing bytes independently of mapped guest addresses.
//!
//! ```
//! use wasm86_machine::Machine;
//! use wasm86_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments};
//!
//! let target = Compiler::new(ExecutionProfile::Real16).target();
//! let cpu = CpuState { eip: 0x100, segments: Segments::real_mode(), ..CpuState::default() };
//! let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
//! let machine = Machine::new(target, cpu, 2 * 1024 * 1024, map)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![forbid(unsafe_code)]

use std::fmt;
use wasm86_x86::{CpuState, ExecutionProfile, ExecutionTarget, PhysicalMemoryMap};

/// Validated initial CPU state, backing capacity and physical mappings.
pub struct Machine {
    target: ExecutionTarget,
    cpu: CpuState,
    memory_size: u64,
    map: PhysicalMemoryMap,
}

impl Machine {
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
        if !target.profile.is_compatible_with(&cpu.segments) {
            return Err(ConfigurationError::IncompatibleCpu);
        }
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
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedProfile => "physical machine configuration requires real mode",
            Self::IncompatibleCpu => "CPU segment caches do not satisfy the execution target",
            Self::InvalidMemorySize => "memory_size exceeds the 4-GiB backing address space",
            Self::InsufficientMemory => "physical mappings exceed memory_size",
        })
    }
}

impl std::error::Error for ConfigurationError {}

//! Initial machine configuration and scoped access to live guest state.
//!
//! [`MachineConfig`] describes the initial CPU, backing size and fixed physical
//! map. Execution backends provide storage; [`Machine`] borrows that storage to
//! expose the current CPU and [`Memory`] with the machine's access policy.
//!
//! ```
//! use relocade_machine::MachineConfig;
//! use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments};
//!
//! let target = Compiler::new(ExecutionProfile::Real16).target();
//! let cpu = CpuState { eip: 0x100, segments: Segments::real_mode(), ..CpuState::default() };
//! let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
//! let config = MachineConfig::new(target, cpu, 2 * 1024 * 1024, map)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Live access reads only the requested CPU fields. Use a snapshot explicitly
//! when saving or inspecting the complete CPU, including floating-point state.
//!
//! ```
//! # use relocade_machine::Machine;
//! # fn update(machine: &mut Machine<'_>) -> Result<(), Box<dyn std::error::Error>> {
//! let ax = machine.cpu().ax();
//! machine.memory().write(0x200, &ax.to_le_bytes())?;
//! machine.cpu().set_ax(ax.wrapping_add(1));
//! let saved = machine.cpu().snapshot();
//! machine.cpu().restore(&saved);
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod config;
mod cpu;
mod memory;

pub use config::{ConfigurationError, MachineConfig};
pub use cpu::Cpu;
pub use memory::{Memory, MemoryError, ReadMemory};
use relocade_x86::ExecutionTarget;

/// Transfers to the CPU and guest memory supplied by the runtime. CPU offsets
/// are relative to the start of `CpuState` in the execution context.
/// Writes take effect immediately; implementations must not cache a CPU snapshot.
/// Backing transfers receive ranges already checked against configured capacity.
pub trait MachineStorage {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]);
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]);
    fn read_backing(&self, offset: u32, bytes: &mut [u8]);
    fn write_backing(&mut self, offset: u32, bytes: &[u8]);
}

/// Scoped access to the stopped CPU and guest memory. Both views read and write
/// execution storage directly.
pub struct Machine<'a, S: MachineStorage + ?Sized = dyn MachineStorage + 'a> {
    config: &'a MachineConfig,
    storage: &'a mut S,
}

impl<'a, S: MachineStorage + ?Sized> Machine<'a, S> {
    /// Binds access to initialized storage. The CPU must be compatible with
    /// `config`'s execution target; the map and backing capacity must also match.
    pub fn new(config: &'a MachineConfig, storage: &'a mut S) -> Self {
        Self { config, storage }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.config.execution_target()
    }

    pub fn cpu(&mut self) -> Cpu<'_, S> {
        Cpu::new(self.config, self.storage)
    }

    pub fn memory(&mut self) -> Memory<'_, S> {
        Memory::new(self.config, self.storage)
    }
}
